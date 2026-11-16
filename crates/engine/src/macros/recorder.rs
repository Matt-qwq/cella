//! The recording buffer: the commands run while a recording is on, each with the sheet and
//! selection it ran in (adoption of upstream's `record.rs` and `macros/recorder.rs`).
//!
//! Only top-level commands that the journal keeps are offered to the buffer — a command that runs
//! other commands (`data.subtotal`, a query load) is one step, so playing the recording back does
//! not run the inner ones twice. Secrets are left out exactly as the journal leaves them out
//! ([`crate::cmd::journal_params`]), because a recording is saved inside the workbook.
//!
//! At most [`RECORD_LIMIT`] steps of at most [`RECORD_MAX_BYTES`] of JSON are kept: a recording is a
//! script, and a script holds at most [`crate::macros::MAX_RUN_STEPS`] steps of at most 64 KiB of
//! params each, so a longer recording is cut at [`Recorder::dropped`] and the stop reports it.
//!
//! ## What is not recorded, and why
//!
//! - `automate.*`: the recorder's own control (`recordActions` starting or stopping a recording)
//!   and the script store itself (`newScript`, `editStep`, `runScript```…```). Replaying any of
//!   them would write scripts or start a recording inside a run.
//! - `macro.*`: the engine's own control in the same way (`macro.record`, `macro.run`,
//!   `macro.debug`); `macro.run` in particular is a run inside a run.
//! - `ext.*`: an extension is a command list of its own; recording `ext.run` would replay the
//!   extension, not what it did.
//! - `edit.undo` / `edit.redo`: they act on the live history of the session that replays them
//!   (inside one undo group, undoing takes back the run's own steps), so what they did cannot be
//!   replayed. Upstream denies them in a macro for the same reason.
//!
//! **A selection command is recorded.** Upstream can leave the person's `selection.set` out because
//! its recorded step carries the selection it ran with and puts it back before the step; our steps
//! are `{command, params}` and carry no such field, so a step that takes its range from the
//! selection (`home.bold` with no `range`) would replay against whatever is selected then. Instead
//! the recording *materializes* the selection: where a command ran with another sheet or another
//! selection than the one before it, a `sheet.activate` / `selection.set` step is added before it
//! (and runs of them collapse to the last), so the steps stand on their own when replayed.

use serde::Serialize;
use serde_json::{Value as Json, json};

use crate::{DocState, Selection};

/// The most steps one recording keeps: the most a script may hold, so a finished recording always
/// fits the store it is saved into.
pub const RECORD_LIMIT: usize = crate::macros::MAX_RUN_STEPS;

/// The most bytes (steps serialized as JSON) one recording keeps: the size a script's whole step
/// list may take in the defined-name store, so the recording can always be saved.
pub const RECORD_MAX_BYTES: usize = crate::cmd::extra::MAX_SCRIPT_STEPS_BYTES;

/// Commands never recorded, with the reason.
pub const NOT_RECORDED_PREFIXES: &[(&str, &str)] = &[
    ("automate.", "the recorder's own control and the script store"),
    ("macro.", "the macro engine's own control"),
    ("ext.", "an extension is a command list of its own"),
];

/// Single commands never recorded, with the reason.
pub const NOT_RECORDED: &[(&str, &str)] = &[
    ("edit.undo", "undo acts on the history of the session that replays it"),
    ("edit.redo", "redo acts on the history of the session that replays it"),
];

/// Why `id` is not recorded, or `None` when it is.
pub fn why_not_recorded(id: &str) -> Option<&'static str> {
    if let Some((_, why)) = NOT_RECORDED_PREFIXES.iter().find(|(p, _)| id.starts_with(p)) {
        return Some(why);
    }
    NOT_RECORDED.iter().find(|(d, _)| *d == id).map(|(_, why)| *why)
}

/// One recorded command and where it ran.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct RecordedStep {
    /// The document the command ran on (0 when none was open).
    pub doc: u64,
    /// That document's active sheet (index and name) just before the command ran.
    pub sheet: usize,
    pub sheet_name: String,
    /// That document's selection just before the command ran.
    pub selection: Selection,
    /// The command id and its parameters, without secrets.
    pub id: String,
    pub params: Json,
}

/// The commands recorded since the recording started, and where it started.
#[derive(Clone, Debug)]
pub struct Recorder {
    /// The document the recording belongs to: a step of another workbook is left out (its cells
    /// and names are not the ones the script is saved into).
    doc: u64,
    sheet: usize,
    selection: Selection,
    steps: Vec<RecordedStep>,
    /// Commands left out because they can't be replayed ([`why_not_recorded`]) or because one
    /// step's params are larger than a script may hold.
    skipped: usize,
    /// Steps past [`RECORD_LIMIT`] or [`RECORD_MAX_BYTES`]. Once one is dropped every later one is
    /// too, so `steps` is always the beginning of what ran, never a sequence with gaps.
    dropped: usize,
    bytes: usize,
}

impl Recorder {
    /// Starts a recording in `d`, remembering the sheet and selection it starts from.
    pub fn new(d: &DocState) -> Recorder {
        Recorder { doc: d.uid, sheet: d.wb.active_sheet, selection: d.selection.clone(), steps: Vec::new(), skipped: 0, dropped: 0, bytes: 0 }
    }

    /// The document the recording belongs to.
    pub fn doc(&self) -> u64 {
        self.doc
    }

    /// Adds a step, or counts it as skipped or dropped; `true` when it was kept.
    pub fn record(&mut self, step: RecordedStep) -> bool {
        if step.doc != self.doc {
            // Another workbook's command: a script is saved into one workbook and runs there.
            self.skipped = self.skipped.saturating_add(1);
            return false;
        }
        if why_not_recorded(&step.id).is_some() {
            self.skipped = self.skipped.saturating_add(1);
            return false;
        }
        let size = serde_json::to_vec(&step).map_or(usize::MAX, |v| v.len());
        if size > crate::cmd::extra::MAX_STEP_PARAMS_BYTES {
            // One step's params are larger than a script may hold, so it could never be saved.
            self.skipped = self.skipped.saturating_add(1);
            return false;
        }
        if self.dropped > 0 || self.steps.len() >= RECORD_LIMIT || self.bytes.saturating_add(size) > RECORD_MAX_BYTES {
            self.dropped = self.dropped.saturating_add(1);
            return false;
        }
        self.bytes = self.bytes.saturating_add(size);
        self.steps.push(step);
        true
    }

    pub fn steps(&self) -> &[RecordedStep] {
        &self.steps
    }

    /// Commands the recording refused ([`why_not_recorded`]).
    pub fn skipped(&self) -> usize {
        self.skipped
    }

    /// Steps the recording cut at its cap.
    pub fn dropped(&self) -> usize {
        self.dropped
    }

    /// The recording as a script's step list (`{command, params}` steps, with the sheet and
    /// selection each command ran with made explicit where they changed), and how many entries
    /// were cut so that the list fits a script: at most [`RECORD_LIMIT`] steps and at most the
    /// store's byte cap. A trailing sheet or selection step the cut would leave dangling goes with
    /// it.
    pub fn to_steps(&self) -> (Vec<Json>, usize) {
        let mut out: Vec<(String, Json)> = Vec::new();
        let (mut sheet, mut selection) = (self.sheet, self.selection.clone());
        for st in &self.steps {
            if st.sheet != sheet {
                out.push(("sheet.activate".into(), json!({ "sheet": st.sheet_name })));
            }
            // A `selection.set` just before already put the selection there.
            let selected = matches!(out.last(), Some((last, _)) if last == "selection.set");
            if (st.selection != selection || st.sheet != sheet) && !selected {
                out.push(("selection.set".into(), selection_params(&st.selection)));
            }
            (sheet, selection) = (st.sheet, st.selection.clone());
            out.push((st.id.clone(), st.params.clone()));
        }
        // Runs of `selection.set` (or `sheet.activate`): the last one wins.
        let mut collapsed: Vec<(String, Json)> = Vec::new();
        for (id, params) in out {
            if matches!(id.as_str(), "selection.set" | "sheet.activate") && collapsed.last().is_some_and(|(last, _)| *last == id) {
                collapsed.pop();
            }
            collapsed.push((id, params));
        }
        // Fit the script the recording is saved into: its step cap and its byte cap.
        let size_of = |(id, params): &(String, Json)| step_json(id, params).to_string().len().saturating_add(1);
        let mut size = 2usize.saturating_add(collapsed.iter().map(size_of).sum::<usize>());
        let mut cut = 0usize;
        while let Some(last) = collapsed.last() {
            if collapsed.len() <= RECORD_LIMIT && size <= crate::cmd::extra::MAX_SCRIPT_STEPS_BYTES {
                break;
            }
            size = size.saturating_sub(size_of(last));
            collapsed.pop();
            cut = cut.saturating_add(1);
        }
        // A sheet or selection step with no command after it says nothing.
        while collapsed.last().is_some_and(|(id, _)| matches!(id.as_str(), "selection.set" | "sheet.activate")) {
            collapsed.pop();
            cut = cut.saturating_add(1);
        }
        (collapsed.into_iter().map(|(command, params)| step_json(&command, &params)).collect(), cut)
    }
}

/// One step in a script's shape.
fn step_json(command: &str, params: &Json) -> Json {
    json!({ "command": command, "params": params })
}

/// `selection.set`'s parameters for `sel`: every area as A1 text, with the active cell.
fn selection_params(sel: &Selection) -> Json {
    let ranges: Vec<String> = sel.ranges.iter().map(gridcraft_core::RangeRef::a1).collect();
    json!({ "range": ranges.join(","), "active": sel.active.a1() })
}

#[cfg(test)]
mod tests {
    use super::*;
    use gridcraft_core::{CellRef, RangeRef};

    #[test]
    fn the_filter_list() {
        for id in [
            "automate.recordActions",
            "automate.runScript",
            "automate.newScript",
            "macro.record",
            "macro.run",
            "macro.debug",
            "ext.run",
            "edit.undo",
            "edit.redo",
        ] {
            assert!(why_not_recorded(id).is_some(), "{id}");
        }
        for id in ["cell.set", "range.setValues", "home.bold", "selection.set", "sheet.activate", "data.sortAscending", "view.zoom"] {
            assert!(why_not_recorded(id).is_none(), "{id}");
        }
    }

    fn step(id: &str, params: Json, sheet: usize, selection: Selection) -> RecordedStep {
        RecordedStep { doc: 1, sheet, sheet_name: format!("Sheet{}", sheet + 1), selection, id: id.into(), params }
    }

    #[test]
    fn the_sheet_and_selection_a_command_ran_with_become_steps() {
        let mut d = crate::DocState::new(gridcraft_model::Workbook::new(), None, "book".into());
        d.uid = 1;
        let start = d.selection.clone();
        let mut rec = Recorder::new(&d);
        // A command with the selection it started with: no selection step.
        assert!(rec.record(step("cell.set", json!({"cell": "A1", "input": "1"}), 0, start.clone())));
        // Another selection, then the same command twice with two selections: both are kept.
        let wide = Selection { ranges: vec![RangeRef::parse("A1:B1").unwrap_or_default()], ..start.clone() };
        assert!(rec.record(step("home.bold", json!({}), 0, wide.clone())));
        let narrow = Selection::at(CellRef::new(2, 0));
        assert!(rec.record(step("cell.set", json!({"cell": "A3", "input": "x"}), 0, narrow.clone())));
        assert!(rec.record(step("cell.set", json!({"cell": "A4", "input": "y"}), 1, narrow)));
        let (out, cut) = rec.to_steps();
        assert_eq!(cut, 0);
        let ids: Vec<&str> = out.iter().filter_map(|s| s["command"].as_str()).collect();
        // The other sheet has its own selection, so activating it says where to select too.
        assert_eq!(
            ids,
            ["cell.set", "selection.set", "home.bold", "selection.set", "cell.set", "sheet.activate", "selection.set", "cell.set"],
            "{out:?}"
        );
        assert_eq!(out[1]["params"]["range"], "A1:B1");
        assert_eq!(out[3]["params"]["active"], "A3");
        assert_eq!(out[5]["params"]["sheet"], "Sheet2");
        assert_eq!(out[6]["params"]["active"], "A3");
    }

    #[test]
    fn the_step_cap_and_the_filter_are_counted() {
        let d = crate::DocState::new(gridcraft_model::Workbook::new(), None, "book".into());
        let mut rec = Recorder::new(&d);
        let sel = d.selection.clone();
        let doc = rec.doc();
        let mut pushed = 0usize;
        for i in 0..(RECORD_LIMIT + 5) {
            let s = RecordedStep {
                doc,
                sheet: 0,
                sheet_name: "Sheet1".into(),
                selection: sel.clone(),
                id: "cell.set".into(),
                params: json!({"cell": "A1", "input": i}),
            };
            if rec.record(s) {
                pushed = pushed.saturating_add(1);
            }
        }
        assert_eq!(pushed, RECORD_LIMIT);
        assert_eq!(rec.dropped(), 5);
        assert_eq!(rec.steps().len(), RECORD_LIMIT);
        assert!(!rec.record(RecordedStep { doc: doc + 1, ..step("cell.set", json!({}), 0, sel.clone()) }), "another workbook");
        assert_eq!(rec.skipped(), 1);
        assert!(!rec.record(step("automate.runScript", json!({"name": "x"}), 0, sel)));
        assert_eq!(rec.skipped(), 2);
    }
}
