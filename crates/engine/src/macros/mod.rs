//! The macro engine under GridCraft's scripts: recording, running, step-through and the trust
//! policy (adoption of upstream's `crates/engine/src/macros/`).
//!
//! **One product, not two.** Upstream keeps macros in `Workbook::macros`, edits them with
//! `macro.save`, and runs them with `macro.run`. GridCraft's scripts are a list of
//! `{command, params}` steps kept as JSON in the defined-name store (`_gc_scripts_<n>`), edited one
//! step at a time by `automate.scriptSteps` / `automate.editStep` / `automate.appendStep` /
//! `automate.removeStep`. This module brings upstream's *engine* — the recorder, the runner, the
//! trust policy and the debugger — under that step list. There is still one store (the scripts
//! stash) and one editor (the `automate.*` step commands); upstream's `macro.*` ids are registered
//! as aliases where they map cleanly (see [`crate::cmd::macros`]) and the ones that would introduce
//! a second store or a second editor (`macro.save`) are not registered at all.
//!
//! What each module is:
//!
//! - [`recorder`]: turns the commands a person (or an agent) runs while recording into steps of a
//!   script, with a step cap and a refusal to record what cannot be replayed;
//! - [`run`]: plays a script through the engine as one undo step, with a step budget and a refusal
//!   at the first failing step (the run is rolled back, and the error names the step);
//! - [`policy`]: what a step may run, gated by the workbook's trust flag;
//! - [`debug`]: `macro.debug`, which runs a script one step at a time inside one open undo group.
//!
//! ## The trust mapping
//!
//! Upstream's `Trust` (`Local` vs `FromFile`) is our per-document `DocState::scripts_trusted`
//! flag, the one `automate.trustScripts` sets and `ext.run` reads: a workbook that arrives from a
//! file already carrying scripts or extensions starts untrusted, and one that has none starts
//! trusted (anything it gains later was written here). So:
//!
//! | upstream | here |
//! | --- | --- |
//! | `Trust::Local`, or a macro the person approved | `scripts_trusted == true` |
//! | `Trust::FromFile`, not approved | `scripts_trusted == false` |
//! | `macro.permissions` (the command ids a macro may reach files with) | not stored: our step list has no field for one, so a step that reads a file needs the workbook to be trusted, and `file.*`, `link.*` and another script are refused whatever the flag says |
//!
//! ## Limits
//!
//! A stored script holds at most [`MAX_RUN_STEPS`] steps of at most 64 KiB of params each (the
//! `automate.*` editor enforces the same numbers, and the stash caps the whole list); a run executes
//! at most [`run::MAX_EXECUTED_STEPS`] steps and passes at most [`run::MAX_PARAM_BYTES`] of params
//! to them; a recording keeps at most [`recorder::RECORD_LIMIT`] steps and
//! [`recorder::RECORD_MAX_BYTES`] of JSON. A script that runs a script is refused by
//! `Session::enter_script` (and by the policy that denies `automate.*` / `macro.*` steps), so
//! nesting is refused rather than recursed.

pub mod debug;
pub mod policy;
pub mod recorder;
pub mod run;

use gridcraft_model::Workbook;
use serde_json::Value as Json;

/// The most steps one script runs: the number the editor keeps in a script
/// ([`crate::cmd::extra::MAX_SCRIPT_STEPS`]), so a stored script is always run whole.
pub const MAX_RUN_STEPS: usize = crate::cmd::extra::MAX_SCRIPT_STEPS;

/// The steps stored in script `name`, or `None` when the workbook has no script of that name.
/// A stored value that is not an array holds no steps, as `automate.scriptSteps` reports it.
pub(crate) fn steps_of(wb: &Workbook, name: &str) -> Option<Vec<Json>> {
    crate::cmd::extra::script_steps_of(wb, name)
}

/// The steps of script `name`, at most [`MAX_RUN_STEPS`] of them.
pub(crate) fn capped_steps(wb: &Workbook, name: &str) -> Option<Vec<Json>> {
    steps_of(wb, name).map(|steps| steps.into_iter().take(MAX_RUN_STEPS).collect())
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_run_limit_is_the_editor_limit() {
        assert_eq!(super::MAX_RUN_STEPS, crate::cmd::extra::MAX_SCRIPT_STEPS);
        assert_eq!(super::MAX_RUN_STEPS, 1_000);
        assert_eq!(super::recorder::RECORD_LIMIT, super::MAX_RUN_STEPS);
    }
}
