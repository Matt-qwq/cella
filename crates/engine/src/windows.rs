//! Windows on open documents: each window has a view of its own.
//!
//! Every open document is shown in one or more windows (View ▸ New Window adds another on the same
//! workbook, and the desktop shell draws each as its own OS window). A window has its own selection,
//! active sheet and per-sheet selection memory — Excel gives each window its own view, so Sheet2
//! with D4 selected in one window does not move the other window.
//!
//! The active window's view is the one every command reads: it lives in its [`DocState`] (`selection`,
//! `sheet_selections`) and in the workbook's active sheet, and each other window keeps its view in
//! [`DocWindow`] until it is activated. Swapping a view in or out never copies the workbook and never
//! makes it dirty: the active sheet is view state (see [`DocState::show_sheet`]), and selections live
//! outside the workbook. Exactly one window of a document is the live one — it holds no parked view.

use crate::{DocState, DocWindow, EngineError, Result, Selection, Session, WindowId};

/// The view one window keeps while another window of its document is the one showing.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WindowView {
    pub selection: Selection,
    /// The active sheet's index, and its name: a sheet inserted, deleted or moved meanwhile in
    /// another window shifts the index, and the name still finds the sheet the window showed.
    pub sheet: usize,
    pub sheet_name: String,
    /// The selection each sheet was left with (Excel's per-sheet selection memory, per window).
    pub sheet_selections: Vec<Selection>,
}

impl WindowView {
    /// The view a document is showing right now.
    fn capture(d: &DocState) -> WindowView {
        WindowView {
            selection: d.selection.clone(),
            sheet: d.wb.active_sheet,
            sheet_name: d.wb.active().map(|s| s.name.clone()).unwrap_or_default(),
            sheet_selections: d.sheet_selections.clone(),
        }
    }

    /// Puts this view back into its document.
    fn apply(self, d: &mut DocState) {
        let sheets = &d.wb.sheets;
        let same = sheets.get(self.sheet).is_some_and(|s| s.name == self.sheet_name);
        let at = if same { self.sheet } else { sheets.iter().position(|s| s.name == self.sheet_name).unwrap_or(self.sheet) };
        d.show_sheet(at.min(sheets.len().saturating_sub(1)));
        d.selection = self.selection;
        d.sheet_selections = self.sheet_selections;
    }
}

impl DocState {
    /// Makes sheet `i` the active one as view state, the way `sheet.activate` does: no undo step,
    /// and a document with nothing unsaved is left as clean as it was.
    pub(crate) fn show_sheet(&mut self, i: usize) {
        if i >= self.wb.sheets.len() || i == self.wb.active_sheet {
            return;
        }
        let dirty = self.is_dirty();
        // A clean document shares its workbook snapshot with `saved`, so the copy goes in and the
        // two share again; with unsaved work the workbook is the only owner and the index changes
        // in place. Either way the document's clean/dirty state does not change.
        std::sync::Arc::make_mut(&mut self.wb).active_sheet = i;
        if !dirty {
            self.saved = self.wb.clone();
        }
    }
}

impl Session {
    /// Every window of every open document, in window-list order: the documents in the order they
    /// were opened, each document's windows in the order they were made. View ▸ Switch Windows
    /// lists them in this order, and `window`/`index` parameters on the window commands index it.
    pub fn windows(&self) -> Vec<(usize, DocWindow)> {
        self.documents().iter().enumerate().flat_map(|(i, d)| d.windows.iter().map(move |w| (i, w.clone()))).collect()
    }

    /// How many windows are open; every open document has at least one.
    pub fn window_count(&self) -> usize {
        self.documents().iter().map(|d| d.windows.len()).sum()
    }

    /// The document a window shows, as an index into [`Session::documents`].
    pub fn window_doc(&self, window: WindowId) -> Option<usize> {
        self.documents().iter().position(|d| d.windows.iter().any(|w| w.id == window))
    }

    /// The window whose view the active document shows: the window the user last worked in, or —
    /// when that id now belongs to another document (or went with it) — its live window.
    pub fn active_window(&self) -> Option<WindowId> {
        let d = self.active()?;
        self.active_window.filter(|id| d.windows.iter().any(|w| w.id == *id)).or_else(|| live_window(d))
    }

    /// What a window shows: its own sheet and selection, or the document's for the live window.
    /// `None` when no such window is open.
    pub fn window_view(&self, window: WindowId) -> Option<WindowView> {
        let d = self.documents().get(self.window_doc(window)?)?;
        let w = d.windows.iter().find(|w| w.id == window)?;
        Some(w.view.clone().unwrap_or_else(|| WindowView::capture(d)))
    }

    /// Focuses a window: its view becomes the one its document shows (the view the document showed
    /// is parked in the window that held it), and with it the document the window shows. False when
    /// no such window is open.
    pub fn focus_window(&mut self, window: WindowId) -> bool {
        let Some(i) = self.window_doc(window) else { return false };
        let Some(d) = self.docs.get_mut(i) else { return false };
        // The window that is showing the document now, found before its view is taken above: the
        // incoming window becomes free of a view itself, and would be mistaken for it.
        let live = d.windows.iter().position(|w| w.view.is_none());
        // The window's own view, unless it is the live one already.
        if let Some(incoming) = d.windows.iter_mut().find(|w| w.id == window).and_then(|w| w.view.take()) {
            if let Some(at) = live {
                let now = WindowView::capture(d);
                if let Some(w) = d.windows.get_mut(at) {
                    w.view = Some(now);
                }
            }
            incoming.apply(d);
        }
        self.set_focus(window, i);
        true
    }

    /// Adds a window to the active document (View ▸ New Window) and focuses it: the same workbook,
    /// its undo history and its save, shown in a window of its own. Both windows start out on the
    /// sheet and selection the document shows; from then on each keeps its own.
    pub fn add_window(&mut self) -> Result<WindowId> {
        let window = DocWindow::new();
        let id = window.id;
        let d = self.doc_mut()?;
        // The window that shows the document keeps a copy of what it shows; the new window takes
        // the live view over.
        let view = WindowView::capture(d);
        if let Some(w) = d.windows.iter_mut().find(|w| w.view.is_none()) {
            w.view = Some(view);
        }
        d.windows.push(window);
        let doc = self.active_index();
        self.set_focus(id, doc);
        Ok(id)
    }

    /// Closes one window; the last window of a document closes the document with it (unsaved
    /// changes are the caller's business: `view.closeWindow` asks first). Returns whether the
    /// document went too.
    pub fn close_window(&mut self, window: WindowId) -> bool {
        let Some(i) = self.window_doc(window) else { return false };
        let Some(d) = self.docs.get_mut(i) else { return false };
        let at = d.windows.iter().position(|w| w.id == window);
        let live = at.is_some_and(|at| d.windows.get(at).is_some_and(|w| w.view.is_none()));
        if let Some(at) = at {
            d.windows.remove(at);
        }
        if d.windows.is_empty() {
            if self.active_window == Some(window) {
                self.active_window = None;
            }
            self.close_document(i);
            return true;
        }
        if live {
            // The document was showing this window's view: the next window hands its own over, so
            // closing the window the user worked in finds the other one where it left it.
            let incoming = d.windows.iter_mut().find_map(|w| w.view.take());
            if let Some(view) = incoming {
                view.apply(d);
            }
        }
        if self.active_window == Some(window) {
            // The document's live window takes the focus, wherever the closed one stood.
            match live_window(d) {
                Some(next) => self.set_focus(next, i),
                None => self.active_window = None,
            }
        }
        false
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
        if self.active_window() == Some(window)
            && let Some(next) = self.next_visible_window(window)
        {
            self.focus_window(next);
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
        let all: Vec<DocWindow> = self.documents().iter().flat_map(|d| d.windows.iter().cloned()).collect();
        let at = all.iter().position(|w| w.id == window)?;
        (1..=all.len()).filter_map(|step| all.get((at + step) % all.len())).find(|w| !w.hidden).map(|w| w.id)
    }

    /// Records window `window` of document `doc` as the one the user works in.
    pub(crate) fn set_focus(&mut self, window: WindowId, doc: usize) {
        self.active_window = Some(window);
        if doc < self.documents().len() {
            self.set_active(doc);
        }
    }
}

/// The window of a document whose view the document itself holds (exactly one, while it is open).
fn live_window(d: &DocState) -> Option<WindowId> {
    d.windows.iter().find(|w| w.view.is_none()).or_else(|| d.windows.first()).map(|w| w.id)
}
