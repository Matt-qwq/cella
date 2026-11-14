//! PivotTables built on the data model, in a part of our own: `xl/gridcraft/pivots.json`.
//!
//! A report over a worksheet range is an Excel PivotTable and is written as one. A report over the
//! **data model** is not: its cache would have to point at `xl/model/*`, the Power Pivot part Excel
//! writes as a compressed binary this implementation keeps but does not author. Writing a cache
//! that claims to be one would be a guess that corrupts a real workbook, so a model report is kept
//! here instead — its definition, by sheet — and the cells it wrote stay ordinary cells, which is
//! what Excel and every other reader sees either way.
//!
//! The rules are the queries part's rules: no relationship, one `<Default Extension="json">`
//! declared only when the part is written, the model is the truth (no model reports, no part), and a
//! broken part is one warning, never a failed open.
//!
//! Why this one uses `serde_json` while the queries part is hand-written: a `PivotTable` is sixty
//! fields across nested structures, and a second, hand-written definition of it would be a copy that
//! drifts. The serde shape *is* the format here, and tests prove the bytes are it.

use gridcraft_model::{PivotSourceKind, PivotTable, Workbook};
use serde::{Deserialize, Serialize};

use crate::IoError;
use crate::read::Ctx;
use crate::write::Out;

/// The part the model's PivotTables are written to.
pub(crate) const PART: &str = "xl/gridcraft/pivots.json";
/// Its extension and content type. Whichever of our parts is written first declares the default;
/// the content-type map keeps the first one for an extension, so there is never a duplicate.
pub(crate) const EXTENSION: &str = "json";
pub(crate) const CONTENT_TYPE: &str = "application/json";

/// The most this reader will decompress and parse: a report definition per sheet, each a few KB.
const MAX_PART_BYTES: u64 = 16 * 1024 * 1024;

/// A report as the part holds it: which sheet carries it, and the definition.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Saved {
    sheet: String,
    pivot: PivotTable,
}

/// Whether a report is one of ours to keep: built on the model, and computed (a carried report has
/// its own parts and needs nothing from us).
fn is_model_pivot(pt: &PivotTable) -> bool {
    pt.source_kind == PivotSourceKind::DataModel && pt.read_only_reason.is_none()
}

/// Reads the model reports and attaches each to the sheet it names. A sheet that is gone takes its
/// report with it (there is nowhere to put it), and one warning says so.
pub(crate) fn read(cx: &mut Ctx<'_>, wb: &mut Workbook) {
    let bytes = match cx.pkg.read_limited(PART, MAX_PART_BYTES) {
        Ok(Some(bytes)) => bytes,
        Ok(None) => return,
        Err(IoError::TooLarge(_)) => {
            cx.warn(format!("{PART}: the part is larger than {} MB and was not read", MAX_PART_BYTES / (1024 * 1024)));
            return;
        }
        Err(e) => {
            cx.warn(format!("{PART}: the part could not be read ({e})"));
            return;
        }
    };
    let saved: Vec<Saved> = match serde_json::from_slice(&bytes) {
        Ok(v) => v,
        Err(e) => {
            let msg = e.to_string();
            let short: String = msg.chars().take(200).collect();
            cx.warn(format!("{PART}: {short}; the workbook has no model PivotTables"));
            return;
        }
    };
    for s in saved.into_iter().take(1_000) {
        let Some(si) = wb.sheet_index(&s.sheet) else {
            cx.warn(format!("{PART}: the sheet \"{}\" is gone, so its PivotTable was not restored", s.sheet));
            continue;
        };
        if !is_model_pivot(&s.pivot) {
            continue;
        }
        if wb.sheet(si).is_some_and(|sh| sh.pivots.iter().any(|p| p.name.eq_ignore_ascii_case(&s.pivot.name))) {
            continue;
        }
        let mut pt = s.pivot;
        if pt.id == 0 || wb.sheets.iter().any(|sh| sh.pivots.iter().any(|p| p.id == pt.id)) {
            let max_id = wb.sheets.iter().flat_map(|sh| sh.pivots.iter().map(|p| p.id)).max().unwrap_or(0);
            pt.id = max_id.saturating_add(1);
        }
        if let Some(sh) = wb.sheet_mut(si) {
            sh.pivots.push(pt);
        }
    }
}

/// Writes the model reports the workbook holds, or nothing when it holds none.
pub(crate) fn write(wb: &Workbook, out: &mut Out) {
    let mut saved: Vec<Saved> = Vec::new();
    for sh in &wb.sheets {
        for pt in sh.pivots.iter().filter(|p| is_model_pivot(p)) {
            saved.push(Saved { sheet: sh.name.clone(), pivot: pt.clone() });
        }
    }
    if saved.is_empty() {
        return;
    }
    let Ok(bytes) = serde_json::to_vec(&saved) else { return };
    out.part(PART, None, bytes);
    out.default_type(EXTENSION, CONTENT_TYPE);
}
