//! The GridCraft document model: workbooks, sheets, a copy-on-write cell store, interned
//! styles, defined names, tables, conditional formats, validation, comments, charts and print
//! settings. Pure data; calculation lives in `gridcraft-calc`.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod b64;
pub mod cell;
mod cellmap;
pub mod data_model;
pub mod equation;
pub mod features;
pub mod line;
pub mod password;
pub mod pivot_item;
pub mod preserved;
pub mod query;
pub mod sheet;
mod slicer;
pub mod store;
pub mod style;
pub mod workbook;

pub use cell::{Cell, CellPicture, Formula};
pub use data_model::{DataModel, MAX_MEASURES, MAX_MODEL_CELLS, MAX_MODEL_TABLES, MAX_RELATIONSHIPS, Measure, ModelTable, Relationship};
pub use equation::Equation;
pub use features::*;
pub use line::{FilterHidden, LineAxis, LineMap};
pub use password::PasswordHash;
pub use preserved::{Preserved, PreservedPart, PreservedRel, PreservedSheet, PreservedXml};
pub use query::{
    HttpFormat, MAX_NAME, MAX_STEPS, Query, QueryAggregate, QueryColumnType, QueryFilterOp, QueryLoad, QuerySource, QueryStep, query_name_is_free,
};
pub use sheet::{DEFAULT_COL_WIDTH, DEFAULT_ROW_HEIGHT, LineIndex, Sheet, SheetKind, SheetViewMode, Visibility, points_to_chars};
pub use slicer::{DEFAULT_SLICER_STYLE, PivotSlicer, PivotTimeline, SLICER_STYLES, SlicerItem, SlicerSort, SlicerValue, TimeUnit, TimelineRange};
pub use store::{BandBatch, CellStore, Compacted, Packed, PageCacheStats, SpillOptions};
pub use style::{
    Alignment, BorderLine, BorderStyle, Borders, Color, Fill, Font, HAlign, NumFmt, PatternType, ReadingOrder, Style, StyleId, StyleTable, Theme,
    Underline, VAlign, VertAlign,
};
pub use workbook::{CalcMode, CalcSettings, DefinedName, DocProps, PhoneticProps, PhoneticRun, PhoneticString, ShowObjects, Workbook, WorkbookView};

/// Font used for new workbooks. Only a name: renderers substitute an available open font
/// (e.g. the metric-compatible Carlito) when it isn't installed.
pub const DEFAULT_FONT: &str = "Calibri";
pub const DEFAULT_FONT_SIZE: f32 = 11.0;
