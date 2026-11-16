//! The GridCraft engine façade.
//!
//! Every user-visible action is a command with a stable id (`cell.set`, `home.bold`,
//! `data.sortAscending`…) and JSON parameters. The egui UI, the CLI, the control channel and MCP
//! all go through [`Session::execute`], so every action is scriptable and journaled.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

pub mod catalog;
pub mod cf;
mod chart_svg;
mod clip_html;
pub mod cmd;
pub mod connect;
pub mod display;
pub mod fill;
pub mod io;
pub mod links;
pub mod outline;
pub mod paper;
pub mod pivot;
pub mod pivot_calc;
pub mod sample;
pub mod selection;
pub mod tables;

use std::sync::Arc;

use gridcraft_calc::Calc;
use gridcraft_core::{CellRef, RangeRef};
use gridcraft_model::Workbook;
use serde::Serialize;
use serde_json::Value as Json;

pub use cmd::{CommandInfo, CommandSpec, command_specs, find_command, register_test_command};
pub use gridcraft_calc as calc;
pub use gridcraft_core as core;
pub use gridcraft_formula as formula;
pub use gridcraft_model as model;
pub use gridcraft_numfmt as numfmt;
pub use selection::Selection;

#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("unknown command `{0}`")]
    UnknownCommand(String),
    #[error("command `{0}` is not available right now: {1}")]
    Disabled(String, String),
    #[error("invalid parameters for `{cmd}`: {msg}")]
    BadParams { cmd: String, msg: String },
    #[error("no workbook open")]
    NoDocument,
    #[error("{0}")]
    Other(String),
    #[error("internal error in `{0}` (the workbook was kept as it was): {1}")]
    Internal(String, String),
}

pub type Result<T> = std::result::Result<T, EngineError>;

#[derive(Clone, Debug)]
pub struct HistoryEntry {
    pub label: String,
    pub wb: Arc<Workbook>,
    pub selection: Selection,
    /// Estimate of storage this snapshot retains beyond the next workbook version.
    pub retained_bytes: usize,
}

/// An undo group opened with [`Session::group_begin`]. Hand it back to [`Session::group_end`]
/// (keep the changes as one undo step) or [`Session::group_rollback`] (undo them all).
#[derive(Debug, PartialEq, Eq)]
#[must_use = "an undo group stays open until it is ended or rolled back"]
pub struct GroupToken {
    id: u64,
    doc: u64,
}

impl GroupToken {
    /// The uid of the document the group belongs to.
    pub fn doc(&self) -> u64 {
        self.doc
    }
}

/// An open undo group: what runs on `doc` until it ends becomes one undo step.
#[derive(Clone, Debug)]
struct OpenGroup {
    id: u64,
    doc: u64,
    label: String,
    /// Undo depth when the group began; the group's steps are cut back to it.
    start: usize,
    /// The workbook as it was when the group began: what the step (or a rollback) restores.
    wb: Arc<Workbook>,
    selection: Selection,
    /// The redo stack when the group began (a rollback puts it back).
    redo: Vec<HistoryEntry>,
}

/// The clipboard: a block of cells (with formulas and styles) or marching-ants reference.
#[derive(Clone, Debug)]
pub struct Clipboard {
    /// Source workbook snapshot, sheet and range.
    pub wb: Arc<Workbook>,
    pub sheet: usize,
    pub range: RangeRef,
    /// The range as copied, before whole rows/columns were trimmed to the used range
    /// (Insert Copied Cells inserts whole lines for a row or column copy).
    pub selected: RangeRef,
    pub cut: bool,
    /// Document uid of the source (a cut only moves within the same workbook).
    pub doc_uid: u64,
    /// Plain text (tab-separated) for the system clipboard.
    pub text: String,
    /// A multi-area copy: the areas as selected (for the marquee). `wb` then holds them slid
    /// together into one block at `range`. Empty for a single-range copy.
    pub areas: Vec<RangeRef>,
}

/// In-cell edit mode as shown in the status bar.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub enum Mode {
    #[default]
    Ready,
    Enter,
    Edit,
    Point,
}

/// Identifies a window: a view onto a document. View ▸ New Window adds one, and the desktop
/// shell draws each as its own OS window. The engine keeps the list so the window menu, the CLI,
/// MCP and the control channel can all name one window of one workbook.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WindowId(pub u64);

/// One window onto a document. Every window of a document shows the same workbook and shares its
/// undo history and its save; hiding or closing one leaves the others alone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DocWindow {
    pub id: WindowId,
    /// Hidden with View ▸ Hide; shown again with View ▸ Unhide.
    pub hidden: bool,
}

static NEXT_WINDOW: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

impl DocWindow {
    fn new() -> DocWindow {
        DocWindow { id: WindowId(NEXT_WINDOW.fetch_add(1, std::sync::atomic::Ordering::Relaxed)), hidden: false }
    }
}

#[derive(Clone, Debug)]
pub struct DocState {
    pub wb: Arc<Workbook>,
    pub calc: Calc,
    pub selection: Selection,
    pub undo: Vec<HistoryEntry>,
    pub redo: Vec<HistoryEntry>,
    pub path: Option<String>,
    pub saved: Arc<Workbook>,
    pub revision: u64,
    pub uid: u64,
    pub title: String,
    /// The windows showing this document, in the order they were opened; never empty while the
    /// document is open (closing its last window closes the document).
    pub windows: Vec<DocWindow>,
    /// Per-sheet selection memory.
    pub sheet_selections: Vec<Selection>,
    /// Allow Edit Ranges unlocked with their password in this session: (sheet name, title).
    pub unlocked_ranges: Vec<(String, String)>,
    /// The format `path`'s content was recognised as when its extension doesn't name one (an
    /// XLSX called `budget.dat`): Save writes it back in that format.
    pub content_kind: Option<io::FileKind>,
    /// The password an encrypted document was opened with, kept so an ordinary Save writes the
    /// protection back instead of silently dropping it. `None` for an unencrypted document; never
    /// serialized, never in the journal, and never in a message.
    pub password: Option<String>,
    /// The encryption scheme the document's file was written with (Agile unless a Standard file
    /// was opened), so an ordinary Save writes the same scheme back. An `encryption` param on a
    /// save overrides it.
    pub encryption: io::EncryptionScheme,
    /// Whether the scripts stored in this workbook may run (`automate.runScript`). A workbook
    /// that arrives already carrying scripts (opened from a file) starts untrusted until the
    /// user trusts it in this session (`automate.trustScripts`); one with no scripts starts
    /// trusted, since any script it gains later was written here.
    pub scripts_trusted: bool,
    /// History entries the byte cap has dropped in this document (either stack). Undoing or
    /// redoing everything then stops short of the state it started from, which the fuzz harness
    /// has to know to compare fairly.
    pub history_dropped: u64,
}

static NEXT_UID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
const UNDO_LIMIT: usize = 100;
/// Past this many entries the journal drops its oldest half.
const JOURNAL_MAX_ENTRIES: usize = 10_000;
/// Past this many bytes (entries serialized as JSON) the journal drops its oldest entries until
/// it is under half. Entries keep their full params so they replay (recorded scripts,
/// `engine.journal`), and a `file.open` holds the whole file as base64: without a size cap a
/// few opens of a large workbook keep tens of MB alive.
pub(crate) const JOURNAL_MAX_BYTES: usize = 64 * 1024 * 1024;
const UNDO_BYTES_LIMIT: usize = 128 * 1024 * 1024;

fn history_retained_bytes(older: &Workbook, newer: &Workbook) -> usize {
    let mut bytes = std::mem::size_of::<Workbook>();
    for (index, sheet) in older.sheets.iter().enumerate() {
        // Moving or inserting sheets must not charge unchanged sheets at a new index.
        if newer.sheets.iter().any(|next| Arc::ptr_eq(sheet, next)) {
            continue;
        }
        bytes = bytes.saturating_add(std::mem::size_of::<gridcraft_model::Sheet>());
        let empty = gridcraft_model::CellStore::new();
        let next = newer.sheets.get(index).map_or(&empty, |next| &next.cells);
        bytes = bytes.saturating_add(sheet.cells.retained_bytes_against(next));
        // Line metadata is chunked the same way: a snapshot shares every chunk the next state did
        // not touch, and a sheet with a million formatted rows is charged its unshared chunks.
        let empty_rows = gridcraft_model::LineMap::new(gridcraft_model::LineAxis::Rows);
        let empty_cols = gridcraft_model::LineMap::new(gridcraft_model::LineAxis::Cols);
        let next_sheet = newer.sheets.get(index);
        bytes = bytes.saturating_add(sheet.rows.retained_bytes_against(next_sheet.map_or(&empty_rows, |next| &next.rows)));
        bytes = bytes.saturating_add(sheet.cols.retained_bytes_against(next_sheet.map_or(&empty_cols, |next| &next.cols)));
    }
    bytes
}

/// Pushes an undo entry, dropping the oldest ones past the byte budget; returns how many went.
fn push_history(history: &mut Vec<HistoryEntry>, label: String, wb: Arc<Workbook>, next: &Workbook, selection: Selection) -> usize {
    push_history_with_budget(history, label, wb, next, selection, UNDO_BYTES_LIMIT)
}

fn push_history_with_budget(
    history: &mut Vec<HistoryEntry>,
    label: String,
    wb: Arc<Workbook>,
    next: &Workbook,
    selection: Selection,
    byte_budget: usize,
) -> usize {
    let retained_bytes = history_retained_bytes(&wb, next);
    history.push(HistoryEntry { label, wb, selection, retained_bytes });
    let mut total = history.iter().fold(0usize, |sum, entry| sum.saturating_add(entry.retained_bytes));
    let mut dropped = 0;
    // Always preserve the latest action, even if that one edit exceeds the budget.
    while history.len() > 1 && (history.len() > UNDO_LIMIT || total > byte_budget) {
        total = total.saturating_sub(history.remove(0).retained_bytes);
        dropped += 1;
    }
    dropped
}

impl DocState {
    pub fn new(mut wb: Workbook, path: Option<String>, title: String) -> DocState {
        let mut calc = Calc::new();
        calc.recalc_on_open(&mut wb);
        // Automation that came with the file — a script or an extension — starts disabled.
        let scripts_trusted = !cmd::extra::has_scripts(&wb) && !cmd::ext::has_extensions(&wb);
        let wb = Arc::new(wb);
        let sel = Selection::at(wb.active().map(|s| s.view_active).unwrap_or_default());
        DocState {
            saved: wb.clone(),
            wb,
            calc,
            selection: sel,
            undo: vec![],
            redo: vec![],
            path,
            revision: 1,
            uid: NEXT_UID.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            title,
            windows: vec![DocWindow::new()],
            sheet_selections: vec![],
            unlocked_ranges: vec![],
            content_kind: None,
            password: None,
            encryption: io::EncryptionScheme::default(),
            scripts_trusted,
            history_dropped: 0,
        }
    }
    pub fn is_dirty(&self) -> bool {
        !Arc::ptr_eq(&self.wb, &self.saved)
    }
    pub fn sheet_index(&self) -> usize {
        self.wb.active_sheet
    }
    /// Records the view state that is saved with the file (`sheetView topLeftCell`, the active
    /// cell): the first visible cell of the scrolling area per sheet index in `tops`, and the
    /// active sheet's active cell. View state never makes the workbook dirty.
    pub fn store_view(&mut self, tops: &[(usize, CellRef)]) {
        let active = self.selection.active;
        let si = self.wb.active_sheet;
        let changed = tops.iter().any(|(i, c)| self.wb.sheets.get(*i).is_some_and(|sh| sh.view_top_left != *c))
            || self.wb.sheets.get(si).is_some_and(|sh| sh.view_active != active);
        if !changed {
            return;
        }
        let dirty = self.is_dirty();
        let wb = Arc::make_mut(&mut self.wb);
        for (i, c) in tops {
            if let Some(sh) = wb.sheets.get_mut(*i)
                && sh.view_top_left != *c
            {
                Arc::make_mut(sh).view_top_left = *c;
            }
        }
        if let Some(sh) = wb.sheets.get_mut(si)
            && sh.view_active != active
        {
            Arc::make_mut(sh).view_active = active;
        }
        if !dirty {
            self.saved = self.wb.clone();
        }
    }
    pub fn display_title(&self) -> String {
        self.path
            .as_deref()
            .and_then(|p| std::path::Path::new(p).file_name())
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| self.title.clone())
    }
}

/// Engine-side preferences.
#[derive(Clone, Debug, Serialize, serde::Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Prefs {
    /// After pressing Enter, move the selection: "down", "right", "up", "left", "none".
    pub enter_direction: String,
    pub autocomplete: bool,
    pub user_name: String,
    pub default_font: String,
    pub default_font_size: f32,
    pub sheets_in_new_workbook: usize,
    pub r1c1: bool,
    /// Words added with Add to Dictionary.
    pub user_dictionary: Vec<String>,
    /// View › Show › Formula Bar (an application option in Excel, not saved in the file).
    pub formula_bar: bool,
}

impl Default for Prefs {
    fn default() -> Self {
        Prefs {
            enter_direction: "down".into(),
            autocomplete: true,
            user_name: "GridCraft User".into(),
            default_font: gridcraft_model::DEFAULT_FONT.into(),
            default_font_size: gridcraft_model::DEFAULT_FONT_SIZE,
            sheets_in_new_workbook: 1,
            r1c1: false,
            user_dictionary: vec![],
            formula_bar: true,
        }
    }
}

/// Requests from commands to the UI (open a dialog, show a message…).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub enum UiRequest {
    Dialog(String, Json),
    Message(String),
    /// Start editing the active cell (F2) with optional initial text.
    EditCell(Option<String>),
    /// Put a defined name at the caret of the cell being edited (Paste Names), or start `=name`
    /// in the active cell when nothing is being edited.
    InsertName(String),
    OpenUrl(String),
}

/// Width in points of one line of text drawn in a cell font, measured by a front end that has
/// the font the grid draws with. With `exact` false the front end may answer with a cheaper
/// measure that is never narrower (AutoFit asks for it on columns with many distinct values).
/// Commands call it while they run, so a front end must not run commands while holding a lock
/// the measure takes (egui's: see `theme::cell_text_width` in the UI crate).
pub type TextWidth = Arc<dyn Fn(&str, &model::Font, bool) -> f32 + Send + Sync>;

#[derive(Default)]
pub struct Session {
    docs: Vec<DocState>,
    active: usize,
    pub clipboard: Option<Clipboard>,
    pub prefs: Prefs,
    pub mode: Mode,
    /// Commands run so far, without their passwords ([`cmd::journal_params`]).
    pub journal: Vec<(String, Json)>,
    /// Serialized size of each journal entry, measured once when it is pushed, and their sum.
    journal_sizes: Vec<usize>,
    journal_bytes: usize,
    pub ui_requests: Vec<UiRequest>,
    /// Format painter source style (sticky when double-clicked).
    pub format_painter: Option<(Arc<Workbook>, usize, RangeRef, bool)>,
    /// Find state (last search).
    pub last_find: Option<Json>,
    /// View › Show › Ruler is off (rulers in Page Layout view; an application option).
    pub hide_ruler: bool,
    /// Journal position where action recording started.
    pub recording: Option<usize>,
    /// The window the user last worked in (see DocWindow); `None` before the first document and
    /// after the document it belonged to closed. Read it through Session::active_window, which
    /// falls back to the active document's first window instead of trusting a stale id.
    active_window: Option<WindowId>,
    /// Draw tab tool: "" (select), "pen" or "eraser"; pen colour and width.
    pub draw_tool: String,
    pub draw_color: String,
    pub draw_width: f32,
    /// Review › Show Comments: the Comments pane is open.
    pub comments_pane: bool,
    /// Review › Notes › Show All Notes: every note is displayed. An application display setting
    /// (like Excel's comment indicator option), so it's not saved in the file.
    pub show_all_notes: bool,
    /// Measures cell text for AutoFit Column Width. Without it (CLI, MCP) AutoFit uses
    /// [`cmd::format::approx_text_width`].
    pub text_width: Option<TextWidth>,
    /// How many commands are running. A command that works by calling others (`data.subtotal`
    /// inserts rows and sets cells) is one user action: only the outermost one takes an undo
    /// snapshot, which is also what keeps a 24-group Subtotal from filling the history.
    command_depth: u32,
    /// Undo groups open on their documents ([`Session::group_begin`]). While one is open the
    /// commands running there add no undo steps of their own: the group makes one when it ends.
    groups: Vec<OpenGroup>,
    /// The next group id. A token carries it, so an ended group's token is refused instead of
    /// ending someone else's group.
    next_group: u64,
}

impl Session {
    pub fn new() -> Session {
        Session::default()
    }
    pub fn documents(&self) -> &[DocState] {
        &self.docs
    }
    pub fn active_index(&self) -> usize {
        self.active
    }
    pub fn set_active(&mut self, i: usize) {
        if i < self.docs.len() {
            self.active = i;
        }
    }
    pub fn active(&self) -> Option<&DocState> {
        self.docs.get(self.active)
    }
    pub fn active_mut(&mut self) -> Option<&mut DocState> {
        self.docs.get_mut(self.active)
    }
    pub fn doc(&self) -> Result<&DocState> {
        self.active().ok_or(EngineError::NoDocument)
    }
    pub fn doc_mut(&mut self) -> Result<&mut DocState> {
        self.active_mut().ok_or(EngineError::NoDocument)
    }
    pub fn add_document(&mut self, d: DocState) -> usize {
        self.docs.push(d);
        self.active = self.docs.len() - 1;
        self.active
    }
    pub fn close_document(&mut self, i: usize) {
        if i < self.docs.len() {
            // An undo group belongs to its document: it goes with it, so no token outlives its
            // workbook and no group keeps a closed document's snapshot alive.
            if let Some(gone) = self.docs.get(i).map(|d| d.uid) {
                self.groups.retain(|g| g.doc != gone);
            }
            self.docs.remove(i);
            if self.active >= self.docs.len() {
                self.active = self.docs.len().saturating_sub(1);
            }
            // The window the closed document showed went with it.
            if self.active_window.and_then(|w| self.window_doc(w)).is_none() {
                self.active_window = self.active().and_then(|d| d.windows.first().map(|w| w.id));
            }
        }
    }

    /// Every window of every open document, in window-list order: the documents in the order they
    /// were opened, each document's windows in the order they were made. View ▸ Switch Windows
    /// lists them in this order, and `window`/`index` parameters on the window commands index it.
    pub fn windows(&self) -> Vec<(usize, DocWindow)> {
        self.docs.iter().enumerate().flat_map(|(i, d)| d.windows.iter().map(move |w| (i, *w))).collect()
    }

    /// How many windows are open; every open document has at least one.
    pub fn window_count(&self) -> usize {
        self.docs.iter().map(|d| d.windows.len()).sum()
    }

    /// The document a window shows, as an index into [`Session::documents`].
    pub fn window_doc(&self, window: WindowId) -> Option<usize> {
        self.docs.iter().position(|d| d.windows.iter().any(|w| w.id == window))
    }

    /// The window the user last worked in: the active document's focused window, or its first one
    /// when the stored window now belongs to another document (or closed with it).
    pub fn active_window(&self) -> Option<WindowId> {
        let d = self.active()?;
        let focused = self.active_window.filter(|id| d.windows.iter().any(|w| w.id == *id));
        focused.or_else(|| d.windows.first().map(|w| w.id))
    }

    /// Focuses a window, and with it the document it shows; false when no such window is open.
    pub fn focus_window(&mut self, window: WindowId) -> bool {
        match self.window_doc(window) {
            Some(i) => {
                self.active = i;
                self.active_window = Some(window);
                true
            }
            None => false,
        }
    }

    /// Adds a window to the active document (View ▸ New Window) and focuses it: the same workbook,
    /// its undo history and its save, shown in a window of its own.
    pub fn add_window(&mut self) -> Result<WindowId> {
        let window = DocWindow::new();
        self.doc_mut()?.windows.push(window);
        self.active_window = Some(window.id);
        Ok(window.id)
    }

    /// Closes one window; the last window of a document closes the document with it (unsaved
    /// changes are the caller's business: `view.closeWindow` asks first). Returns whether the
    /// document went too.
    pub fn close_window(&mut self, window: WindowId) -> bool {
        let Some(i) = self.window_doc(window) else { return false };
        let mut last = false;
        if let Some(d) = self.docs.get_mut(i) {
            d.windows.retain(|w| w.id != window);
            last = d.windows.is_empty();
        }
        if self.active_window == Some(window) {
            self.active_window = None;
        }
        if last {
            self.close_document(i);
        }
        last
    }

    /// Hides a window and focuses the next visible one. The last visible window stays visible and
    /// nothing happens, as View ▸ Hide has always behaved with one window: the app always shows
    /// something.
    pub fn hide_window(&mut self, window: WindowId) -> Result<()> {
        if self.window_doc(window).is_none() {
            return Err(EngineError::Other("no such window".into()));
        }
        if self.windows().iter().filter(|(_, w)| !w.hidden).count() <= 1 {
            return Ok(());
        }
        if let Some(d) = self.window_doc(window).and_then(|i| self.docs.get_mut(i))
            && let Some(w) = d.windows.iter_mut().find(|w| w.id == window)
        {
            w.hidden = true;
        }
        if self.active_window() == Some(window) {
            let next = self.next_visible_window(window);
            if let Some(next) = next {
                self.focus_window(next);
            }
        }
        Ok(())
    }

    /// Shows a hidden window again and focuses it; false when no such window is open.
    pub fn unhide_window(&mut self, window: WindowId) -> bool {
        if let Some(d) = self.window_doc(window).and_then(|i| self.docs.get_mut(i))
            && let Some(w) = d.windows.iter_mut().find(|w| w.id == window)
        {
            w.hidden = false;
        }
        self.focus_window(window)
    }

    /// The next visible window after `window` in window-list order, wrapping around.
    fn next_visible_window(&self, window: WindowId) -> Option<WindowId> {
        let all: Vec<DocWindow> = self.docs.iter().flat_map(|d| d.windows.iter().copied()).collect();
        let at = all.iter().position(|w| w.id == window)?;
        (1..=all.len()).filter_map(|step| all.get((at + step) % all.len())).find(|w| !w.hidden).map(|w| w.id)
    }
    /// A new blank workbook titled `BookN`, its Normal style in the default font from the
    /// preferences (File ▸ Options ▸ when creating new workbooks).
    pub fn new_workbook(&mut self) -> usize {
        let n = (1..).find(|n| !self.docs.iter().any(|d| d.title == format!("Book{n}") && d.path.is_none())).unwrap_or(1);
        let font = self.prefs.default_font.clone();
        let size = self.prefs.default_font_size;
        let mut wb = Workbook::with_default_font(&font, size);
        for _ in 1..self.prefs.sheets_in_new_workbook.clamp(1, 255) {
            let name = wb.next_sheet_name();
            wb.sheets.push(Arc::new(gridcraft_model::Sheet::new(name)));
        }
        self.add_document(DocState::new(wb, None, format!("Book{n}")))
    }

    /// Journals a command, then trims the oldest entries past either cap. A recording in progress
    /// keeps pointing at the same entry (the steps the trim dropped are lost).
    fn push_journal(&mut self, id: &str, params: Json) {
        let size = |e: &(String, Json)| serde_json::to_vec(e).map_or(0, |v| v.len());
        // `journal` is public: if it was changed from outside, measure it again.
        if self.journal_sizes.len() != self.journal.len() {
            self.journal_sizes = self.journal.iter().map(size).collect();
            self.journal_bytes = self.journal_sizes.iter().sum();
        }
        let entry = (id.to_string(), params);
        let entry_size = size(&entry);
        self.journal.push(entry);
        self.journal_sizes.push(entry_size);
        self.journal_bytes = self.journal_bytes.saturating_add(entry_size);
        let len = self.journal.len();
        let mut n = if len > JOURNAL_MAX_ENTRIES { len - JOURNAL_MAX_ENTRIES / 2 } else { 0 };
        let mut kept = self.journal_bytes.saturating_sub(self.journal_sizes.iter().take(n).sum());
        if self.journal_bytes > JOURNAL_MAX_BYTES {
            // The newest entry stays even when it is larger than the cap on its own.
            while kept > JOURNAL_MAX_BYTES / 2 && n + 1 < len {
                kept = kept.saturating_sub(self.journal_sizes.get(n).copied().unwrap_or(0));
                n += 1;
            }
        }
        if n > 0 {
            self.journal.drain(..n);
            self.journal_sizes.drain(..n);
            self.journal_bytes = kept;
            self.recording = self.recording.map(|r| r.saturating_sub(n));
        }
    }

    pub fn commands(&self) -> Vec<CommandInfo> {
        command_specs().iter().map(|c| c.info(self)).collect()
    }

    /// Runs a command by id. Undoable commands record a history step when the workbook changed.
    /// Panics inside commands are caught and reported; the workbook is restored.
    pub fn execute(&mut self, id: &str, params: Json) -> Result<Json> {
        let spec = find_command(id).ok_or_else(|| EngineError::UnknownCommand(id.to_string()))?;
        if let Err(why) = (spec.enabled)(self) {
            return Err(EngineError::Disabled(id.to_string(), why));
        }
        let before = self.active().map(|d| (d.uid, d.wb.clone(), d.selection.clone()));
        let first_group = self.next_group;
        let outer = self.command_depth == 0;
        self.command_depth = self.command_depth.saturating_add(1);
        let params = if params.is_null() { Json::Object(Default::default()) } else { params };
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| (spec.run)(self, &params)));
        self.command_depth = self.command_depth.saturating_sub(1);
        let result = match result {
            Ok(r) => r,
            Err(p) => {
                let msg =
                    p.downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| p.downcast_ref::<String>().cloned()).unwrap_or_else(|| "panic".into());
                // A command that panicked leaves none of the undo groups it opened behind: the
                // restore below takes back what it changed.
                self.groups.retain(|g| g.id < first_group);
                // Restore the workbook as it was.
                if let Some((uid, wb, sel)) = &before
                    && let Some(d) = self.docs.iter_mut().find(|d| d.uid == *uid)
                {
                    d.wb = wb.clone();
                    d.selection = sel.clone();
                    d.calc.rebuild(&d.wb);
                }
                return Err(EngineError::Internal(id.to_string(), msg));
            }
        };
        // A command that fails leaves none of its own undo groups open (a `?` between
        // `group_begin` and `group_end`): each is rolled back as `group_rollback` does.
        if result.is_err() {
            self.rollback_groups_from(first_group);
        }
        if outer && spec.journal && result.is_ok() {
            self.push_journal(id, cmd::journal_params(&params));
        }
        // Inside an undo group the group owns the step: the commands running there add none of
        // their own (the group makes one when it ends), and the redo history waits for it too.
        let grouped = before.as_ref().is_some_and(|(uid, ..)| self.in_group(*uid));
        if outer
            && spec.undoable
            && let Some((uid, wb, sel)) = before
            && let Some(d) = self.docs.iter_mut().find(|d| d.uid == uid)
            && !Arc::ptr_eq(&d.wb, &wb)
        {
            if grouped {
                d.revision += 1;
            } else {
                d.history_dropped += push_history(&mut d.undo, spec.label.trim_end_matches('…').to_string(), wb, &d.wb, sel) as u64;
                d.redo.clear();
                d.revision += 1;
            }
        }
        result
    }

    /// Runs `f` as one undo step labelled `label` on the active document: whatever commands `f`
    /// runs, one Undo brings back the workbook and selection as they were before. If `f` fails or
    /// panics, every change it made is rolled back and its error (or the panic) is passed on.
    /// Groups nest; the outermost one makes the step.
    pub fn grouped<R>(&mut self, label: &str, f: impl FnOnce(&mut Session) -> Result<R>) -> Result<R> {
        let token = self.group_begin(label)?;
        let doc = token.doc();
        // A panic in `f` rolls the group back before it goes on, so a caller that catches it
        // (outside `execute`, which drops the groups a panicking command opened) finds no group
        // left open.
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(self))) {
            Ok(Ok(r)) => match self.group_end(token) {
                Ok(_) => Ok(r),
                // `f` closed the group's document (`file.close`), and the group went with it:
                // there is nothing left to undo and nothing left open.
                Err(_) if !self.docs.iter().any(|d| d.uid == doc) => Ok(r),
                Err(e) => Err(e),
            },
            Ok(Err(e)) => {
                // The error from `f` is the one to report. A rollback fails only when the group
                // has already ended, and then there is nothing left to undo.
                let _ = self.group_rollback(token);
                Err(e)
            }
            Err(panic) => {
                let _ = self.group_rollback(token);
                std::panic::resume_unwind(panic)
            }
        }
    }

    /// Opens an undo group on the active document that can span several calls (a macro, a script,
    /// a debugger that runs one step per call). Until [`Session::group_end`] or
    /// [`Session::group_rollback`], undoable commands on that document add no undo steps of their
    /// own and Undo and Redo are unavailable there; the group becomes one step.
    pub fn group_begin(&mut self, label: &str) -> Result<GroupToken> {
        let d = self.doc()?;
        let g = OpenGroup {
            id: self.next_group,
            doc: d.uid,
            label: label.trim_end_matches('…').to_string(),
            start: d.undo.len(),
            wb: d.wb.clone(),
            selection: d.selection.clone(),
            redo: d.redo.clone(),
        };
        let token = GroupToken { id: g.id, doc: g.doc };
        self.next_group = self.next_group.saturating_add(1);
        self.groups.push(g);
        Ok(token)
    }

    /// Ends an undo group. If the workbook changed, the group becomes one undo step (costed the
    /// way any step is, from the snapshot it began with to the workbook it leaves), unless it is
    /// nested in another open group on the same document, which then owns the step. Groups on
    /// that document opened inside it and still open end with it. Returns whether the workbook
    /// changed.
    pub fn group_end(&mut self, token: GroupToken) -> Result<bool> {
        let g = self.take_group(&token)?;
        let outer = self.in_group(g.doc);
        let Some(d) = self.docs.iter_mut().find(|d| d.uid == g.doc) else { return Ok(false) };
        // Steps pushed directly while the group was open fold into the group's step.
        d.undo.truncate(g.start);
        let changed = !Arc::ptr_eq(&d.wb, &g.wb);
        if changed && !outer {
            d.history_dropped += push_history(&mut d.undo, g.label, g.wb, &d.wb, g.selection) as u64;
            d.redo.clear();
            d.revision += 1;
        }
        Ok(changed)
    }

    /// Ends an undo group by putting back the workbook, selection, undo and redo history as they
    /// were when it began. A group inside one rolled back before it is already gone.
    pub fn group_rollback(&mut self, token: GroupToken) -> Result<()> {
        let g = self.take_group(&token)?;
        let Some(d) = self.docs.iter_mut().find(|d| d.uid == g.doc) else { return Ok(()) };
        if !Arc::ptr_eq(&d.wb, &g.wb) {
            d.wb = g.wb;
            d.calc.rebuild(&d.wb);
        }
        d.selection = g.selection;
        d.undo.truncate(g.start);
        d.redo = g.redo;
        d.revision += 1;
        Ok(())
    }

    /// Whether the token's undo group is still open (nothing has ended or rolled it back).
    pub fn group_is_open(&self, token: &GroupToken) -> bool {
        self.groups.iter().any(|g| g.id == token.id)
    }

    /// Whether an undo group is open on the document with this uid.
    pub fn in_group(&self, doc: u64) -> bool {
        self.groups.iter().any(|g| g.doc == doc)
    }

    /// Removes the token's group and any group on the same document opened after it.
    fn take_group(&mut self, token: &GroupToken) -> Result<OpenGroup> {
        let ended = || EngineError::Other("that undo group has already ended".into());
        let at = self.groups.iter().position(|g| g.id == token.id).ok_or_else(ended)?;
        let mut later = self.groups.split_off(at).into_iter();
        let g = later.next().ok_or_else(ended)?;
        self.groups.extend(later.filter(|o| o.doc != g.doc));
        Ok(g)
    }

    /// Rolls back every open undo group numbered `first` or later (the groups a failed command
    /// opened), outermost first on each document.
    fn rollback_groups_from(&mut self, first: u64) {
        let opened: Vec<(u64, u64)> = self.groups.iter().filter(|g| g.id >= first).map(|g| (g.id, g.doc)).collect();
        for (id, doc) in opened {
            // A group inside one rolled back before it is already gone.
            if self.groups.iter().any(|g| g.id == id) {
                let _ = self.group_rollback(GroupToken { id, doc });
            }
        }
    }

    pub fn run(&mut self, id: &str, params: Json) -> std::result::Result<Json, String> {
        self.execute(id, params).map_err(|e| e.to_string())
    }

    pub fn take_ui_requests(&mut self) -> Vec<UiRequest> {
        std::mem::take(&mut self.ui_requests)
    }

    /// Runs a command for a caller with no UI (MCP, CLI): queued UI requests are dropped, and a command that only
    /// queued a file dialog because it got no file (`file.open`, `file.saveAs`, `file.save` on a workbook with no
    /// file yet, `data.getData`) is an error instead of a silent success.
    pub fn execute_headless(&mut self, id: &str, params: Json) -> Result<Json> {
        let r = self.execute(id, params);
        let requests = self.take_ui_requests();
        let asked_for_file = requests.iter().any(|q| matches!(q, UiRequest::Dialog(name, _) if name == "open" || name == "saveAs"));
        if r.is_ok() && asked_for_file {
            let what = match id {
                "file.open" => "`path` or `base64`",
                "data.getData" | "data.fromTextCsv" => "`path` or `text`",
                _ => "`path`",
            };
            let msg = format!("missing {what}: programmatic calls never open dialogs");
            return Err(EngineError::BadParams { cmd: id.to_string(), msg });
        }
        r
    }
}

/// Convenience: the active workbook's active sheet.
pub fn active_sheet(s: &Session) -> Option<&gridcraft_model::Sheet> {
    s.active().and_then(|d| d.wb.active())
}

pub fn cell(r: u32, c: u32) -> CellRef {
    CellRef::new(r, c)
}

#[cfg(test)]
mod tests;
