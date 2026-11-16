//! Form controls (Insert ▸ Controls): buttons that run a script or a command, and sliders and
//! spin buttons that hold a whole number and write it to a linked cell.
//!
//! A control is a floating object of its own (it is not a shape), anchored to the grid like a
//! picture and painted above every other object. Its value lives both here and in its linked cell;
//! the value commands keep the two in step.

use gridcraft_core::CellRef;
use serde::{Deserialize, Serialize};

use crate::features::Anchor;

/// What a form control on the sheet is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ControlKind {
    #[default]
    Button,
    Slider,
    Spin,
}

impl ControlKind {
    /// The kind's name in the window: Button, Slider, Spin Button.
    pub fn label(self) -> &'static str {
        match self {
            ControlKind::Button => "Button",
            ControlKind::Slider => "Slider",
            ControlKind::Spin => "Spin Button",
        }
    }

    /// The wire name the commands use: `button`, `slider`, `spinButton`.
    pub fn wire(self) -> &'static str {
        match self {
            ControlKind::Button => "button",
            ControlKind::Slider => "slider",
            ControlKind::Spin => "spinButton",
        }
    }
}

/// The largest value a slider or spin button takes (Excel's scroll bar limit). Values are whole
/// numbers from 0 to this.
pub const CONTROL_MAX: u32 = 30_000;

/// The most form controls one sheet holds (a bound on a hostile file and on a runaway loop).
pub const MAX_CONTROLS: usize = 10_000;

/// The longest name a button's script may have (a stored script name's own cap is checked where
/// scripts are stored; this only keeps a hostile file from carrying a huge one).
pub const MAX_MACRO_NAME: usize = 255;

/// What a button runs when it is pressed.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ControlAction {
    /// Runs the workbook's script `name` (GridCraft's scripts hold no inputs or outputs of their
    /// own: a script reads the selection and writes cells).
    Macro {
        name: String,
        #[serde(default)]
        input: MacroInput,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        output: Option<String>,
    },
    /// Runs one GridCraft command with fixed parameters. The parameters are JSON object text
    /// (`{"cell":"B2"}`; empty is none): the model crate holds no JSON value, so the engine reads
    /// it. A command action has no place in an .xlsx file, so a save keeps only macro actions.
    Command {
        id: String,
        #[serde(default)]
        params: String,
    },
}

/// The range a button's script runs on, as a file records it (Excel's `FmlaMacro` and the
/// assigned range). GridCraft's scripts read the selection, so a fixed input is kept for the
/// round trip but not used when the button is pressed.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MacroInput {
    /// The selection when the button is pressed.
    #[default]
    Selection,
    /// Always this range (`B2:D9` or `Data!B2:D9`).
    Fixed(String),
}

/// A form control (button, slider, spin button) floating over the sheet.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FormControl {
    pub id: u32,
    pub kind: ControlKind,
    pub anchor: Anchor,
    /// The name shown in the Selection Pane (`Button 3`); empty means the kind and id.
    pub name: String,
    /// The caption of a button.
    pub text: String,
    /// The linked cell as reference text: `$B$2` on the control's own sheet, or `Sheet2!$B$2`.
    pub link: Option<String>,
    /// The current value of a slider or spin button (the linked cell holds it too).
    pub value: u32,
    pub min: u32,
    pub max: u32,
    /// Incremental change: one arrow click or arrow key.
    pub step: u32,
    /// Page change (sliders): a click on the track, or Page Up / Page Down.
    pub page: u32,
    /// A slider lies horizontally (false: vertically).
    pub horizontal: bool,
    /// What a button runs.
    pub action: Option<ControlAction>,
    /// Printed with the sheet.
    pub print: bool,
    /// Locked when the sheet is protected.
    pub locked: bool,
}

impl Default for FormControl {
    fn default() -> Self {
        FormControl {
            id: 0,
            kind: ControlKind::default(),
            anchor: Anchor::default(),
            name: String::new(),
            text: String::new(),
            link: None,
            value: 0,
            min: 0,
            max: 100,
            step: 1,
            page: 10,
            horizontal: true,
            action: None,
            print: true,
            locked: true,
        }
    }
}

impl FormControl {
    /// The name shown for the control: its own, else `Button 3`, `Slider 4`, `Spin Button 5`.
    pub fn display_name(&self) -> String {
        if self.name.trim().is_empty() { format!("{} {}", self.kind.label(), self.id) } else { self.name.clone() }
    }

    /// Checks the numbers of a slider or spin button: 0 ≤ min < max ≤ 30000, 1 ≤ step ≤ max − min,
    /// and (sliders) page ≥ step. Each rule has its own message.
    pub fn check_numbers(&self) -> Result<(), String> {
        if self.kind == ControlKind::Button {
            return Ok(());
        }
        if self.max > CONTROL_MAX {
            return Err(format!("Maximum value can be at most {CONTROL_MAX}."));
        }
        if self.min > CONTROL_MAX {
            return Err(format!("Minimum value must be a whole number from 0 to {CONTROL_MAX}."));
        }
        if self.min >= self.max {
            return Err(format!("Maximum value ({}) must be greater than Minimum value ({}).", self.max, self.min));
        }
        if self.step < 1 {
            return Err("Incremental change must be at least 1.".into());
        }
        let span = self.max - self.min;
        if self.step > span {
            return Err(format!("Incremental change ({}) can be at most Maximum minus Minimum ({span}).", self.step));
        }
        if self.kind == ControlKind::Slider {
            if self.page < self.step {
                return Err(format!("Page change ({}) must be at least the incremental change ({}).", self.page, self.step));
            }
            if self.page > CONTROL_MAX {
                return Err(format!("Page change can be at most {CONTROL_MAX}."));
            }
        }
        Ok(())
    }

    /// `v` clamped to min..max and snapped to the nearest step from min (max stays reachable).
    pub fn snap(&self, v: f64) -> u32 {
        let min = self.min.min(CONTROL_MAX);
        let max = self.max.min(CONTROL_MAX).max(min);
        if !v.is_finite() || v <= f64::from(min) {
            return min;
        }
        if v >= f64::from(max) {
            return max;
        }
        let step = f64::from(self.step.max(1));
        let snapped = (f64::from(min) + ((v - f64::from(min)) / step).round() * step).clamp(f64::from(min), f64::from(max));
        // When the step doesn't divide the range, the maximum is a stop of its own.
        if (f64::from(max) - v).abs() < (snapped - v).abs() { max } else { snapped as u32 }
    }

    /// Whether the control is one that holds a number (a slider or a spin button).
    pub fn is_number(&self) -> bool {
        self.kind != ControlKind::Button
    }

    /// The cell the control names in `link`, read as `cell` (the reference text's body, quotes
    /// and `$` included). `None` without a link, or when it isn't a cell reference.
    pub fn link_cell(&self) -> Option<CellRef> {
        let text = self.link.as_deref()?.trim().trim_start_matches('=');
        let body = gridcraft_core::split_sheet(text).1;
        CellRef::parse(body.trim())
    }
}
