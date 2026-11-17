//! Memory accounting: the live heap bytes a session's document holds, by part.
//!
//! What this answers is "where does the memory go?" for a workbook people actually build — which
//! part of a million-row sheet is the cells, which is the formula text, which is the dependency
//! graph — and what a later representation change has to move. It is arithmetic on lengths,
//! capacities and layouts, not a heap census: see [`MemoryReport`] for exactly what is counted.

use std::mem::size_of;
use std::sync::Arc;

use serde::Serialize;

use gridcraft_calc::recalc::CalcMemory;
use gridcraft_model::{Sheet, StoreMemory, Workbook, btree_index_bytes, hash_index_bytes};

use crate::{DocState, Session, history_retained_bytes};

/// One sheet's part of a [`MemoryReport`]: its cell store, and the metadata that lives beside it.
#[derive(Clone, Debug, Default, Serialize)]
pub struct SheetMemory {
    pub name: String,
    /// Everything the cells hold (see [`StoreMemory`]).
    pub cells: StoreMemory,
    /// Row and column metadata (heights, widths, hidden flags), by chunk.
    pub lines: usize,
    /// The sheet's other content, at its inline size: merges, conditional formats, validation,
    /// tables, charts, drawings, comments, hyperlinks, pivots, slicers, form controls.
    pub objects: usize,
}

impl SheetMemory {
    pub fn total(&self) -> usize {
        self.cells.total().saturating_add(self.lines).saturating_add(self.objects)
    }
}

/// The parts of a workbook that are not in a sheet's cell store.
#[derive(Clone, Debug, Default, Serialize)]
pub struct WorkbookMemory {
    /// The interning table: the style vector and the index whose keys are style clones.
    pub styles: usize,
    /// The defined names (their text).
    pub names: usize,
    /// The workbook's own containers: the sheet list, custom lists, cell styles, phonetics,
    /// queries, the data model, the document properties and the VBA blob (see [`MemoryReport`]
    /// for what is skipped).
    pub other: usize,
}

impl WorkbookMemory {
    pub fn total(&self) -> usize {
        self.styles.saturating_add(self.names).saturating_add(self.other)
    }
}

/// The live heap bytes one document holds, by part: what [`Session::memory_report`] returns and
/// serializes.
///
/// # What is counted
///
/// * [`sheets`](Self::sheets): each sheet's [`StoreMemory`] (the cells, which is where a big
///   workbook's memory is), its row/column metadata and its objects' inline size.
/// * [`workbook`](Self::workbook): the style table, the defined names and the workbook's own
///   containers.
/// * [`calc`](Self::calc): the dependency graph and the calculation state.
/// * [`saved`](Self::saved): the snapshot Save compares the workbook with.
/// * [`undo`](Self::undo) and [`redo`](Self::redo): the history stacks.
/// * [`journal`](Self::journal): the serialized command params the journal keeps for replay.
///
/// The snapshot and the history stacks are reported as their own lines, and each is measured *as
/// what it retains beyond the live workbook* (the figure [`crate::HistoryEntry`] budgets with): a
/// snapshot shares every band the edit did not touch, so charging it in full would count the live
/// workbook twice. A history step the byte budget compressed is charged its deflated pages, not the
/// bands they decode to.
///
/// # What is deliberately skipped
///
/// Allocator slack beyond a `Vec`'s `capacity`; the `Arc` control blocks of bands, sheets and
/// snapshots; the page files on disk *as disk* — a snapshot that keeps its bands as pages is
/// charged their compressed length, which is what it costs to keep the step (see
/// [`crate::HistoryEntry`]); the contents of an object — a table's rows, a chart's series,
/// a shape's text, a picture's bytes, a pivot cache, a preserved (unmodelled) file part (a VBA
/// blob's bytes are counted, its contents are not decoded anywhere); the `Value`s inside an
/// `Arc<Array>` (see [`StoreMemory`]); and the expression trees the graph's nodes point at,
/// which the cells own and charge (see [`gridcraft_calc::recalc::GraphMemory`]).
#[derive(Clone, Debug, Default, Serialize)]
pub struct MemoryReport {
    pub sheets: Vec<SheetMemory>,
    pub workbook: WorkbookMemory,
    pub calc: CalcMemory,
    /// The last-saved workbook, as what it retains beyond the live one.
    pub saved: usize,
    /// The undo stack, as what it retains beyond the live workbook.
    pub undo: usize,
    /// The redo stack, as what it retains beyond the live workbook.
    pub redo: usize,
    /// The journal's serialized params (`engine.journal`), which a replay needs.
    pub journal: usize,
    /// The whole document: the sheets, the workbook's parts, the calculation state, the history.
    pub total: usize,
}

impl Session {
    /// The live heap bytes the active document holds, by part. A session with no document reports
    /// zeroes.
    ///
    /// Costs: one pass over the bands of every sheet, one over each page, and one over the entries
    /// of the live sheet's metadata (see [`StoreMemory`] and
    /// [`gridcraft_calc::recalc::GraphMemory`]); no cell is visited, so this is cheap on a
    /// million-row workbook.
    pub fn memory_report(&self) -> MemoryReport {
        self.active().map_or_else(MemoryReport::default, |doc| report(doc, self.journal_bytes))
    }
}

fn report(doc: &DocState, journal: usize) -> MemoryReport {
    let wb: &Workbook = &doc.wb;
    let sheets: Vec<SheetMemory> = wb
        .sheets
        .iter()
        .map(|sheet| SheetMemory {
            name: sheet.name.clone(),
            cells: sheet.cells.memory(),
            lines: sheet.rows.memory().saturating_add(sheet.cols.memory()),
            objects: sheet_objects(sheet),
        })
        .collect();
    let workbook = WorkbookMemory { styles: wb.styles.memory(), names: names_bytes(wb), other: workbook_other(wb) };
    let calc = doc.calc.memory();
    let saved = history_retained_bytes(&doc.saved, wb);
    let undo = retained(&doc.undo, wb);
    let redo = retained(&doc.redo, wb);
    let mut total =
        workbook.total().saturating_add(calc.total()).saturating_add(saved).saturating_add(undo).saturating_add(redo).saturating_add(journal);
    for sheet in &sheets {
        total = total.saturating_add(sheet.total());
    }
    MemoryReport { sheets, workbook, calc, saved, undo, redo, journal, total }
}

/// What one history stack retains beyond the live workbook. Entries are measured against the live
/// workbook rather than against each other, so the line is exactly "memory the history keeps that
/// the workbook does not share" — the convention [`crate::HistoryEntry::retained_bytes`] uses
/// when it decides what an edit costs.
fn retained(history: &[crate::HistoryEntry], live: &Workbook) -> usize {
    history.iter().fold(0usize, |sum, entry| sum.saturating_add(history_retained_bytes(&entry.wb, live)))
}

fn names_bytes(wb: &Workbook) -> usize {
    let mut bytes = wb.names.capacity().saturating_mul(size_of::<gridcraft_model::DefinedName>());
    for name in &wb.names {
        bytes = bytes.saturating_add(name.name.len()).saturating_add(name.formula.len()).saturating_add(name.comment.len());
    }
    bytes
}

/// The workbook-level containers, at their inline size: the vectors and maps (keys and values),
/// not the text or the rows inside the objects they hold.
fn workbook_other(wb: &Workbook) -> usize {
    let mut bytes = size_of::<Workbook>()
        .saturating_add(wb.sheets.capacity().saturating_mul(size_of::<Arc<Sheet>>()))
        .saturating_add(vec_bytes(&wb.custom_lists))
        .saturating_add(vec_bytes(&wb.cell_styles))
        .saturating_add(vec_bytes(&wb.phonetics))
        .saturating_add(vec_bytes(&wb.queries))
        .saturating_add(vec_bytes(&wb.model.tables))
        .saturating_add(vec_bytes(&wb.model.relationships))
        .saturating_add(vec_bytes(&wb.model.measures))
        .saturating_add(strings_bytes(&wb.props.title))
        .saturating_add(strings_bytes(&wb.props.subject))
        .saturating_add(strings_bytes(&wb.props.author))
        .saturating_add(strings_bytes(&wb.props.company))
        .saturating_add(strings_bytes(&wb.props.keywords))
        .saturating_add(strings_bytes(&wb.props.description));
    if let Some(vba) = &wb.vba_project {
        bytes = bytes.saturating_add(if Arc::strong_count(vba) > 1 { size_of::<Arc<Vec<u8>>>() } else { vba.capacity() });
    }
    bytes
}

/// A sheet's other content, at its inline size: the maps' entries and the vectors' slots. The
/// contents an entry points at (a table's rows, a chart's series, a shape's text, a picture's
/// bytes, a pivot's cache) are not walked — see [`MemoryReport`].
fn sheet_objects(sheet: &Sheet) -> usize {
    size_of::<Sheet>()
        .saturating_add(strings_bytes(&sheet.name))
        .saturating_add(btree_entries(&sheet.cell_pictures))
        .saturating_add(btree_entries(&sheet.spill))
        .saturating_add(btree_entries(&sheet.spill_ranges))
        .saturating_add(btree_entries(&sheet.comments))
        .saturating_add(btree_entries(&sheet.hyperlinks))
        .saturating_add(vec_bytes(&sheet.merges))
        .saturating_add(vec_bytes(&sheet.cond_formats))
        .saturating_add(vec_bytes(&sheet.validations))
        .saturating_add(vec_bytes(&sheet.tables))
        .saturating_add(vec_bytes(&sheet.charts))
        .saturating_add(vec_bytes(&sheet.kept_charts))
        .saturating_add(vec_bytes(&sheet.images))
        .saturating_add(vec_bytes(&sheet.shapes))
        .saturating_add(vec_bytes(&sheet.equations))
        .saturating_add(vec_bytes(&sheet.sparklines))
        .saturating_add(vec_bytes(&sheet.slicers))
        .saturating_add(vec_bytes(&sheet.controls))
        .saturating_add(vec_bytes(&sheet.pivot_slicers))
        .saturating_add(vec_bytes(&sheet.pivot_timelines))
        .saturating_add(vec_bytes(&sheet.pivots))
        .saturating_add(vec_bytes(&sheet.edit_ranges))
}

/// A string's characters, by length (a string nobody grew again is exact).
fn strings_bytes(s: &str) -> usize {
    s.len()
}

/// Heap bytes of a vector's element storage, by capacity.
#[allow(clippy::ptr_arg)] // `capacity` is the point: a slice would report `len`
fn vec_bytes<T>(v: &Vec<T>) -> usize {
    v.capacity().saturating_mul(size_of::<T>())
}

/// Heap bytes of a [`BTreeMap`]'s index, by the entries it holds.
fn btree_entries<K, V>(map: &std::collections::BTreeMap<K, V>) -> usize {
    btree_index_bytes(map.len(), size_of::<(K, V)>())
}

/// Heap bytes of a hash map's table, by its capacity. Beside [`btree_entries`] for the maps a
/// later field may bring; no sheet map is a hash today.
#[allow(dead_code)]
fn hash_entries<K, V>(map: &std::collections::HashMap<K, V>) -> usize {
    hash_index_bytes(map.capacity(), size_of::<(K, V)>())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::Session;

    fn s() -> Session {
        let mut s = Session::new();
        s.new_workbook();
        s
    }

    /// A new workbook is nearly free, and asking for its report never panics.
    #[test]
    fn an_empty_workbook_reports_almost_nothing() {
        let s = s();
        let report = s.memory_report();
        assert_eq!(report.sheets.len(), 1);
        assert!(report.total < 256 * 1024, "{} bytes for a new workbook", report.total);
        assert_eq!(report.calc.graph.nodes, 0, "an empty graph");
        assert_eq!(report.undo, 0);
        assert_eq!(report.redo, 0);
        assert!(report.workbook.styles > 0, "the style table is charged");
        assert!(report.sheets[0].total() > 0);
        // A session with no document at all reports zeroes.
        assert_eq!(Session::new().memory_report().total, 0);
    }

    /// The report is a partition: its total is the parts, and each sheet's is its own.
    #[test]
    fn the_report_total_is_the_sum_of_its_parts() {
        let mut s = s();
        s.execute("cell.set", json!({"cell": "A1", "input": "1"})).unwrap();
        s.execute("cell.set", json!({"cell": "B1", "input": "=A1*2"})).unwrap();
        let report = s.memory_report();
        let sheets: usize = report.sheets.iter().map(|sheet| sheet.total()).sum();
        let parts = sheets
            .saturating_add(report.workbook.total())
            .saturating_add(report.calc.total())
            .saturating_add(report.saved)
            .saturating_add(report.undo)
            .saturating_add(report.redo)
            .saturating_add(report.journal);
        assert_eq!(report.total, parts, "the total is the sum of the lines");
        for sheet in &report.sheets {
            assert_eq!(sheet.total(), sheet.cells.total() + sheet.lines + sheet.objects);
        }
    }

    /// A thousand formulas raise the cell store's formula lines and the graph, and each part stays
    /// in its own line: the graph charges its nodes, the store charges the text and the trees.
    #[test]
    fn a_thousand_formulas_raise_the_cell_and_graph_lines() {
        let mut s = s();
        let before = s.memory_report();
        for row in 1..=1000 {
            s.execute("cell.set", json!({"cell": format!("A{row}"), "input": format!("{row}")})).unwrap();
            s.execute("cell.set", json!({"cell": format!("B{row}"), "input": "=A1+1"})).unwrap();
        }
        let after = s.memory_report();
        let text = after.sheets[0].cells.formula_text.saturating_sub(before.sheets[0].cells.formula_text);
        let trees = after.sheets[0].cells.formula_exprs.saturating_sub(before.sheets[0].cells.formula_exprs);
        // A formula's `Formula` struct (a `String`, the tree pointer, the array range) and its
        // four characters of text: somewhere between 16 and 128 bytes each, never nothing.
        assert!(text >= 1000 * 16, "{text} bytes of formula text for 1000 formulas");
        assert!(text <= 1000 * 128, "{text} bytes of formula text is more than the shape implies");
        assert!(trees > 0, "{trees} bytes of expression trees");
        assert_eq!(s.doc().unwrap().calc.graph.len(), 1000, "one graph node a formula");
        // A node is a few pointer-sized fields: nowhere near a formula's text and tree.
        assert!(after.calc.graph.nodes / 1000 < 512, "{} bytes of graph for 1000 formulas", after.calc.graph.nodes);
        assert!(after.calc.total() > before.calc.total(), "the graph grew");
        assert!(after.total > before.total);
        // The sheet's objects are not where a million formulas' memory is.
        assert!(after.sheets[0].objects < after.total / 2);
    }

    /// An edit leaves a snapshot behind, reported as its own line: it shares every band it did not
    /// touch with the live workbook, so it retains almost nothing and the store is not doubled.
    #[test]
    fn history_is_reported_as_what_it_retains_beyond_the_workbook() {
        let mut s = s();
        for row in 1..=200 {
            s.execute("cell.set", json!({"cell": format!("A{row}"), "input": format!("{row}")})).unwrap();
        }
        let live = s.memory_report();
        assert!(live.undo > 0, "every step retained the band it replaced");
        // Counted in full, the stack would cost the sum of the whole workbook once per step. What
        // is reported is only what each snapshot keeps that the live workbook does not, so the
        // line is a fraction of that.
        let in_full: usize =
            s.doc().unwrap().undo.iter().map(|entry| entry.wb.sheets.iter().map(|sheet| sheet.cells.memory().total()).sum::<usize>()).sum();
        assert!(live.undo < in_full / 2, "{} reported against {in_full} counted in full", live.undo);
        assert!(live.undo < live.total, "the history is a line, not the document");
        // Undo moves the snapshot to the redo stack; the figure stays "what it keeps beyond".
        s.execute("edit.undo", json!({})).unwrap();
        let undone = s.memory_report();
        assert!(undone.redo > 0, "the undone snapshot retains the band the undo replaced");
        assert!(undone.undo < live.undo, "a step left the undo stack");
        s.execute("edit.redo", json!({})).unwrap();
        assert!(s.memory_report().undo > undone.undo);
    }
}
