//! What a script step may run (adoption of upstream's `macros/policy.rs`).
//!
//! A script is a list of commands, so the policy goes by command id and by the parameters that
//! name a file (`path`) or the network (`url`). It keeps a script from saving, opening or closing
//! workbooks, opening web pages, running other scripts or extensions, driving windows, and undoing
//! the run it is a step of — those are refused whatever the workbook's trust flag says. A step
//! that reads a file or goes to the network is refused unless the workbook is trusted in this
//! session ([`crate::DocState::scripts_trusted`], the flag `automate.trustScripts` sets and
//! `ext.run` reads); see the mapping in [`super`].
//!
//! The check is by id and parameter name, so it is only the up-front refusal: a step also runs
//! inside an [`crate::io::ScriptSandbox`] unless the policy approved it for the file system, so a
//! command that touches a file under another parameter name can't either.

use serde_json::Value as Json;

/// Whether a command may run as a script step.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Policy {
    /// Runs as it is.
    Allow,
    /// The workbook must be trusted in this session before the script runs at all; the text says
    /// why it asks ("came with a workbook you opened").
    Ask(String),
    /// Reads a file or goes to the network: allowed only in a trusted workbook. The text says what
    /// it does ("touches a file").
    NeedsPermission(String),
    /// Never runs as a step; the text says why.
    Deny(String),
}

/// The reason [`Policy::Ask`] gives: the same one `automate.allScripts` reports as `trusted: false`.
pub const UNTRUSTED: &str = "came with a workbook you opened";

/// Prefixes of commands that never run as a script step, with the reason. `automate.` and
/// `macro.` are also what makes a script that runs a script impossible instead of recursive.
const DENIED_PREFIXES: &[(&str, &str)] = &[
    ("file.", "scripts don't open, save, close or create workbooks"),
    ("link.", "scripts don't open web pages"),
    ("automate.", "scripts don't record or run other scripts"),
    ("macro.", "scripts don't record, run or debug other scripts"),
    ("ext.", "scripts don't run extensions"),
];

/// Single commands that never run as a script step.
const DENIED: &[(&str, &str)] = &[
    ("edit.undo", "a script is undone as one step after it runs"),
    ("edit.redo", "a script is undone as one step after it runs"),
    ("view.newWindow", "scripts don't open, hide or close windows"),
    ("view.hideWindow", "scripts don't open, hide or close windows"),
    ("view.unhideWindow", "scripts don't open, hide or close windows"),
    ("view.closeWindow", "scripts don't open, hide or close windows"),
    ("window.activate", "scripts don't switch windows"),
];

/// Commands that reach outside the workbook whatever their parameters are: they re-read a query
/// source that may be a file or a URL, so no `path` or `url` in the step's own parameters says so.
const ALWAYS: &[(&str, &str)] = &[
    ("data.fromDatabase", "reads a database file"),
    ("data.fromWeb", "goes to the web"),
    ("data.query.refresh", "reloads a query's source, which may be a file or a web page"),
    ("data.query.load", "loads a query, whose source may be a file or a web page"),
    ("data.query.addStep", "re-runs a query pipeline, whose source may be a file or a web page"),
    ("data.query.setStep", "re-runs a query pipeline, whose source may be a file or a web page"),
];

/// How deep a parameter object is searched for `path`/`url` keys, and how many values are looked
/// at: a hostile step can hold a huge object, and the check must not walk it without a bound.
const SCAN_DEPTH: usize = 8;
const SCAN_BUDGET: usize = 10_000;

/// The policy for running command `id` with `params` as a step of a script in a workbook that is
/// trusted ([`true`]) or arrived from a file ([`false`]).
pub fn script_policy(id: &str, params: &Json, trusted: bool) -> Policy {
    match base_policy(id, params) {
        // A workbook that came from a file runs nothing until it is trusted, whichever step is
        // first: the same refusal `automate.runScript` has always had.
        Policy::Allow | Policy::NeedsPermission(_) if !trusted => Policy::Ask(UNTRUSTED.into()),
        other => other,
    }
}

/// The policy without the trust flag.
fn base_policy(id: &str, params: &Json) -> Policy {
    if let Some((_, why)) = DENIED_PREFIXES.iter().find(|(p, _)| id.starts_with(p)) {
        return Policy::Deny((*why).into());
    }
    if let Some((_, why)) = DENIED.iter().find(|(d, _)| *d == id) {
        return Policy::Deny((*why).into());
    }
    if let Some((_, what)) = ALWAYS.iter().find(|(d, _)| *d == id) {
        return Policy::NeedsPermission((*what).into());
    }
    let mut budget = SCAN_BUDGET;
    if holds_key(params, "path", SCAN_DEPTH, &mut budget) {
        return Policy::NeedsPermission("touches a file".into());
    }
    let mut budget = SCAN_BUDGET;
    if holds_key(params, "url", SCAN_DEPTH, &mut budget) {
        return Policy::NeedsPermission("goes to the network".into());
    }
    Policy::Allow
}

/// Whether `v` holds a non-empty `key` at any depth up to `depth` (a token or a blank string
/// counts: it names a file once the step runs).
fn holds_key(v: &Json, key: &str, depth: usize, budget: &mut usize) -> bool {
    if *budget == 0 || depth == 0 {
        return false;
    }
    *budget = budget.saturating_sub(1);
    match v {
        Json::Object(o) => o.iter().any(|(k, x)| (k == key && !x.is_null()) || holds_key(x, key, depth.saturating_sub(1), budget)),
        Json::Array(a) => a.iter().any(|x| holds_key(x, key, depth.saturating_sub(1), budget)),
        _ => false,
    }
}

/// Whether a step runs without the script sandbox: only a file step the policy approved for a
/// trusted workbook reaches the file system.
pub fn reaches_files(id: &str, params: &Json) -> bool {
    matches!(script_policy(id, params, true), Policy::NeedsPermission(_))
}

/// Why a step is refused whatever the workbook's trust — the text of [`Policy::Deny`], which is
/// what `ext.run` shares (an extension never touches a file either, so a step it would need the
/// file system for is refused there too).
pub fn refused_step(id: &str, params: &Json) -> Option<String> {
    match base_policy(id, params) {
        Policy::Deny(why) => Some(format!("scripts can't run `{id}`: {why}")),
        Policy::NeedsPermission(what) => Some(format!("scripts can't pass `{id}` a file: it {what}")),
        Policy::Allow | Policy::Ask(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_deny_and_permission_lists() {
        let none = json!({});
        for id in [
            "file.save",
            "file.open",
            "file.saveAs",
            "file.close",
            "file.exportPdf",
            "file.print",
            "link.open",
            "automate.runScript",
            "automate.newScript",
            "macro.run",
            "macro.record",
            "macro.debug",
            "ext.run",
            "edit.undo",
            "edit.redo",
            "view.newWindow",
            "view.hideWindow",
            "view.unhideWindow",
            "window.activate",
        ] {
            assert!(matches!(script_policy(id, &none, true), Policy::Deny(_)), "{id}");
        }
        // A step that reads a file or goes to the network needs a trusted workbook.
        for (id, params) in [
            ("data.getData", json!({"path": "/tmp/x.csv"})),
            ("data.fromTextCsv", json!({"path": "/tmp/x.csv"})),
            ("insert.picture", json!({"path": "/tmp/x.png"})),
            ("pageLayout.background", json!({"path": "/tmp/x.png"})),
            ("data.fromDatabase", json!({"path": "/tmp/x.db", "sql": "select 1"})),
            ("data.fromWeb", json!({"url": "https://example.com/x.csv"})),
            ("data.query.add", json!({"source": {"kind": "sqlite", "path": "/tmp/x.db", "sql": "select 1"}})),
            ("data.query.refresh", none.clone()),
        ] {
            assert!(matches!(script_policy(id, &params, true), Policy::NeedsPermission(_)), "{id} {params}");
            assert!(matches!(script_policy(id, &params, false), Policy::Ask(_)), "{id} {params}");
        }
        // Without a path or a url they touch nothing outside the workbook.
        for (id, params) in [
            ("data.getData", json!({"text": "a,b\n1,2"})),
            ("data.fromTextCsv", json!({"text": "a,b\n1,2"})),
            ("data.getData", json!({"path": null})),
            ("cell.set", json!({"cell": "A1", "input": "x"})),
            ("home.bold", none.clone()),
            ("range.setValues", json!({"range": "A1", "values": [[1]]})),
        ] {
            assert_eq!(script_policy(id, &params, true), Policy::Allow, "{id} {params}");
        }
        // Any other command with a path in it is a file step too (a query source, a print target).
        assert!(matches!(script_policy("insert.chart", &json!({"path": "x.png"}), true), Policy::NeedsPermission(_)));
        assert!(matches!(
            script_policy("data.query.setSource", &json!({"source": {"kind": "http", "url": "https://e/x"}}), true),
            Policy::NeedsPermission(_)
        ));
        // An untrusted workbook allows nothing.
        for id in ["cell.set", "home.bold", "range.setValues"] {
            assert!(matches!(script_policy(id, &none, false), Policy::Ask(_)), "{id}");
        }
        assert!(reaches_files("data.getData", &json!({"path": "x"})));
        assert!(!reaches_files("cell.set", &none));
    }

    #[test]
    fn every_listed_command_exists() {
        for id in DENIED_PREFIXES.iter().map(|(p, _)| p).chain(DENIED.iter().map(|(id, _)| id)).chain(ALWAYS.iter().map(|(id, _)| id)) {
            // A prefix is checked through the command it stands for.
            let id = match *id {
                "file." => "file.save",
                "link." => "link.open",
                "automate." => "automate.runScript",
                "macro." => "macro.run",
                "ext." => "ext.run",
                other => other,
            };
            assert!(crate::find_command(id).is_some(), "{id}");
        }
    }
}
