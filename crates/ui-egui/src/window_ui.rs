//! The interface state each window keeps for itself.
//!
//! [`SheetApp`]'s per-window fields (the cell draft, the text-box editor, an open dialog, the
//! Backstage view) hold the state of one window: the engine's active window. Every other window's
//! state is parked in a [`WindowUi`] and swapped in when that window is activated or drawn, the
//! way the engine swaps a window's selection and sheet into its workbook. So a draft, a dialog or
//! the File view stays in the window it belongs to — switching windows shows that window's.

use cella_engine::WindowId;

use crate::{SheetApp, backstage, dialogs, editor, text_box};

/// One window's parked interface state (see the module docs).
#[derive(Default)]
pub struct WindowUi {
    pub(crate) editor: Option<editor::EditState>,
    pub(crate) text_box_editor: Option<text_box::EditState>,
    pub(crate) dialog: Option<dialogs::Dialog>,
    pub(crate) backstage: Option<backstage::Backstage>,
}

impl SheetApp {
    /// Exchanges the per-window fields with `w`. Destructured in full, so a field added to
    /// [`WindowUi`] can't be left out of the swap.
    fn swap_window_ui(&mut self, w: &mut WindowUi) {
        let WindowUi { editor, text_box_editor, dialog, backstage } = w;
        std::mem::swap(&mut self.editor, editor);
        std::mem::swap(&mut self.text_box_editor, text_box_editor);
        std::mem::swap(&mut self.dialog, dialog);
        std::mem::swap(&mut self.backstage, backstage);
    }

    /// The window whose interface state is in the per-window fields.
    pub fn ui_window(&self) -> Option<WindowId> {
        self.ui_window
    }

    /// Brings the per-window fields in line with the engine's active window, after a command (or
    /// another window's frame) named a different one: the previous window's state is parked, the
    /// active one's swapped in (a window never seen before starts fresh). A closed window's state
    /// is dropped.
    pub fn follow_active_window(&mut self) {
        let active = self.session.active_window();
        // A window that closed takes its parked state with it.
        let session = &self.session;
        self.parked.retain(|w, _| session.window_doc(*w).is_some());
        if active == self.ui_window {
            return;
        }
        let Some(previous) = self.ui_window else {
            // The first window: the fields are already its state.
            self.ui_window = active;
            return;
        };
        let mut outgoing = WindowUi::default();
        self.swap_window_ui(&mut outgoing);
        if self.session.window_doc(previous).is_some() {
            self.parked.insert(previous, outgoing);
        }
        if let Some(mut incoming) = active.and_then(|w| self.parked.remove(&w)) {
            self.swap_window_ui(&mut incoming);
        }
        self.ui_window = active;
    }

    /// Makes window `id` the current one with what it shows and with its own interface state: its
    /// sheet and selection, its draft, its dialog and its File view.
    pub fn show_window(&mut self, id: WindowId) {
        self.session.focus_window(id);
        self.follow_active_window();
    }

    /// Whether a window other than the current one has a cell or text-box draft open: work parked
    /// in a window the user has to come back to before it can be committed.
    pub fn parked_window_with_draft(&self) -> Option<WindowId> {
        self.parked.iter().filter(|(_, w)| w.editor.is_some() || w.text_box_editor.is_some()).map(|(id, _)| *id).min()
    }
}
