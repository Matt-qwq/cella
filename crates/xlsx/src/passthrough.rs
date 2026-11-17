//! Pass-through of package content Cella does not model, so that open → edit → save keeps it.
//!
//! On read, the raw bytes of unmodelled parts are kept with their content type and their
//! relationship graph, per owner: the package root (custom document properties, ribbon
//! customisation, add-in task panes), the workbook (custom XML, links to external workbooks with
//! their `<externalReferences>`, data connections, XML maps, its unmodelled `<extLst>` children and
//! the slicer and timeline caches they or a relationship name), each sheet (query tables, cells
//! mapped to XML elements, form controls, ActiveX controls, OLE objects, the sheet background and
//! printer settings, with the sheet's `<picture>`, `<oleObjects>` and `<controls>` XML, the `r:id`
//! of `<pageSetup>`, the legacy VML shapes those controls need and the pictures of the headers and
//! footers, `&G`, with their `<legacyDrawingHF>`; and the sheet's unmodelled `<extLst>` children,
//! such as a timeline) and the parts Cella rewrites (a table's `<extLst>` and the query table
//! attached to it, a PivotTable's `<extLst>` — the slicer cache reference among them — and the
//! `<extLst>` of its cache definition). The workbook also keeps its `<fileVersion>` and the
//! calcFeatures extension, which Excel needs to trust the saved values.
//!
//! On write, kept parts get fresh part names where needed and new relationship ids; the `r:id`s in
//! kept XML are rewritten to match. Kept content is conservative:
//! - Anything whose relationship graph reaches a part Cella reads and rewrites (worksheets,
//!   drawings, charts, tables, PivotTables, comments…) is not kept: it would point at content that
//!   no longer exists in the same form. Slicers, slicer caches, timelines and timeline caches are
//!   not rewritten, so their graphs are kept whole — they are never edited (gap #8).
//! - Extensions Cella writes itself are never kept from the source: the calc-features
//!   extension, the worksheet's data bars, sparklines and x14 validations, and the PivotTable's own
//!   x14 extensions are written from the model, once.
//! - Content a part Cella rewrites owns hangs off that owner's name (`table:<name>`,
//!   `pivot:<name>`, `pivotcache:<name>`), stored in the owning sheet's or workbook's kept content;
//!   its relationships carry the key in their id (see `KS`). A table or PivotTable that is
//!   deleted, renamed or replaced therefore takes its extensions and the parts only they reach
//!   with it, instead of having them attached to another one. An extension whose relationship ids
//!   can't all be resolved is dropped whole.
//! - A slicer or timeline cache that names a PivotTable the workbook no longer has is dropped, with
//!   the slicers and timelines behind it: a filter for a report that is gone is a repair prompt, and
//!   a part nothing relates to is one too.
//! - External relationship targets (the path of a linked workbook) are kept as text and never
//!   opened, followed or refreshed. A relative path to a linked workbook is rewritten when the
//!   workbook is saved into another folder ([`rebase_links`]), so it still names the same file.
//! - Sheet content lives on the sheet: deleting the sheet drops it, Copy Sheet doesn't copy it.
//!   Cell and sheet references inside kept parts (a control's linked cell, a query table's range, a
//!   cell mapped to an XML element, a slicer cache's `pivotCacheId`) are kept as they were, not
//!   adjusted to inserted or deleted rows or reordered sheets.
//! - A PivotTable Cella reads but does not compute (a data-model or OLAP cache) is carried
//!   whole: its own part, its cache definition, its records and the data model behind them, byte
//!   for byte, keyed to the report's name. The workbook's data model (`xl/model/*`) is kept with
//!   the workbook on its own, so it survives even every report built on it being deleted. The
//!   model is never read, only carried.
//! - Not kept: chart `<extLst>` children and chart details, and Strict Open XML files (their
//!   namespaces differ from what we write).
//! - Sizes are bounded: per part, per owner and per workbook.

use crate::IoError;
use crate::package::{self, normalize};
use crate::read::Ctx;
use crate::write::{NS_REL, Out, Rels, XML_DECL};
use crate::xml::{self, esc_attr};
use cella_model::{PivotTable, Preserved, PreservedPart, PreservedRel, PreservedSheet, PreservedXml, Sheet, Visibility, Workbook};
use quick_xml::events::attributes::Attribute;
use quick_xml::events::{BytesStart, Event};
use quick_xml::name::QName;
use quick_xml::{Reader, Writer};
use std::borrow::Cow;
use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt::Write as _;
use std::path::{Component, Path};
use std::sync::Arc;

mod read;
mod rebase;
mod write;

pub use read::KeptReader;
pub use rebase::{SheetKept, rebase_links};
pub use write::{KeptOut, owner_extras, owner_kept, workbook_elements, write_package_rels, write_pivot_parts, write_workbook};
pub(crate) use write::{NameMap, attach, live_parts, part_rels_xml, remap, sheet_names_in, write_parts};

/// Most bytes kept for one workbook.
pub const MAX_KEPT_TOTAL: usize = 256 * 1024 * 1024;
/// Largest single kept part.
pub const MAX_KEPT_PART: u64 = 64 * 1024 * 1024;
/// Most parts kept for one workbook.
const MAX_KEPT_PARTS: usize = 4096;
/// Most relationships read from one rels part.
const MAX_RELS: usize = 10_000;
/// Largest relationships, content-types or VML part read here.
const MAX_META: u64 = 16 * 1024 * 1024;
/// Longest chain of relationships followed from an owner.
const MAX_CHAIN: usize = 16;
/// Largest kept XML fragment of a sheet.
const MAX_FRAGMENT: usize = 4 * 1024 * 1024;
/// Deepest element nesting accepted in a kept fragment.
const MAX_FRAGMENT_DEPTH: usize = 256;
/// Most `extLst` children kept for one owner.
const MAX_EXTS: usize = 256;
/// Most bytes of `extLst` children kept for one owner.
const MAX_EXTS_BYTES: usize = 8 * 1024 * 1024;
/// Separator between an owner key and the source relationship id in [`PreservedRel::id`]. XML 1.0
/// forbids it in a part, so it can't come out of a file and can't be confused with a real id.
const KS: char = '\u{1}';
/// The slot of an `extLst` child of the workbook or a worksheet that Cella does not model.
const EXT_SLOT: &str = "ext";
/// The slot of the workbook's calc-features extension (written by [`workbook_elements`]).
pub(crate) const CALC_SLOT: &str = "calcFeatures";
#[cfg(test)]
thread_local! {
    /// A smaller kept-bytes allowance for tests on this thread.
    pub(crate) static KEPT_TOTAL_FOR_TEST: std::cell::Cell<usize> = const { std::cell::Cell::new(MAX_KEPT_TOTAL) };
}
fn kept_total() -> usize {
    #[cfg(test)]
    {
        KEPT_TOTAL_FOR_TEST.with(std::cell::Cell::get)
    }
    #[cfg(not(test))]
    {
        MAX_KEPT_TOTAL
    }
}
const NS_REL_STRICT: &str = "http://purl.oclc.org/ooxml/officeDocument/relationships";
const CT_VML: &str = "application/vnd.openxmlformats-officedocument.vmlDrawing";
const CT_SLICER_CACHE: &str = "application/vnd.ms-excel.slicerCache+xml";
const CT_TIMELINE_CACHE: &str = "application/vnd.ms-excel.timelineCache+xml";
const CT_SLICER: &str = "application/vnd.ms-excel.slicer+xml";
const CT_TIMELINE: &str = "application/vnd.ms-excel.timeline+xml";
/// Relationship kinds (last segment of the type) kept from the package root.
const PACKAGE_ROOTS: &[&str] = &["custom-properties", "extensibility", "webextensiontaskpanes"];
/// Relationship kinds kept from the workbook without a workbook element. Slicer and timeline
/// caches are related to the workbook; the extensions naming them are kept as well when the file
/// has them.
const WORKBOOK_ROOTS: &[&str] = &["customXml", "connections", "xmlMaps"];
/// Workbook relationship kinds whose parts are kept even when no XML element names them.
const WORKBOOK_CACHES: &[&str] = &["slicerCache", "timelineCache"];
/// Relationship kinds a table part owns that Cella doesn't write (its query table).
const TABLE_OWNED: &[&str] = &["queryTable"];
/// The relationship type that ties a pivot cache definition to the workbook's data model
/// (Power Pivot), and the model part to the workbook when Cella keeps it on its own.
const POWER_PIVOT_KINDS: &[&str] = &["powerPivotData"];
/// The relationships inside a PivotTable's kept content. Cella writes these parts from the
/// model, so they are normally rewritten; a report it does not compute is carried whole instead,
/// and the walk goes through them to the cache, its records and the data model behind them.
const PIVOT_GRAPH: &[&str] = &["pivotTable", "pivotCacheDefinition", "pivotCacheRecords", "powerPivotData"];
/// Relationship kinds kept from a sheet without a sheet element.
const SHEET_ROOTS: &[&str] = &["queryTable", "tableSingleCells"];
/// Sheet relationship kinds that mean the sheet XML may hold elements worth keeping.
const SHEET_ELEMENT_KINDS: &[&str] = &["oleObject", "ctrlProp", "control", "image", "package", "printerSettings"];
/// Attributes of `<fileVersion>` (CT_FileVersion).
const FILE_VERSION_ATTRS: &[&str] = &["appName", "lastEdited", "lowestEdited", "rupBuild", "codeName"];
/// The workbook extension listing the calculation features the saved values were computed with.
const CALC_FEATURES_URI: &str = "{B58B0392-4F1F-4190-BB64-5DF3571DCE5F}";
const NS_CALC_FEATURES: &str = "http://schemas.microsoft.com/office/spreadsheetml/2018/calcfeatures";
/// Worksheet extensions Cella models and writes itself (solid data bars, sparklines, the
/// validations of Excel 2010, and the id that links a data bar to its rule): keeping the source
/// ones too would duplicate them or contradict the model.
const MODELED_SHEET_EXTS: &[&str] =
    &["{78C0D931-6437-407d-A8A1-0F90B4350D7D}", "{05C60535-1F16-4fd2-B633-F4F36F0B64E0}", "{B025F937-C7B1-47D3-B67F-A62EFF666E3E}"];
const MODELED_SHEET_EXT_ELEMENTS: &[&str] = &["conditionalFormattings", "sparklineGroups", "dataValidations"];
/// The PivotTable extensions Cella writes itself (the show-values-as rank, `hideValuesRow`).
const MODELED_PIVOT_EXTS: &[&str] = &[crate::pivot::EXT_X14_DATA_FIELD, crate::pivot::EXT_X14_PIVOT_TABLE];
/// Whether a worksheet `<ext>` holds something the model has no room for. Read with the sheet, so
/// the pass-through only reads the part again (it is the biggest part of the file) when there is
/// something to keep there. The list is the one `keep_exts` uses: the same extensions Cella
/// writes itself are skipped on the way back.
pub(crate) fn sheet_ext_unknown(ext: &xml::El) -> bool {
    let uri = ext.attr("uri").unwrap_or("");
    if MODELED_SHEET_EXTS.iter().any(|m| m.eq_ignore_ascii_case(uri)) {
        return false;
    }
    ext.children.iter().any(|c| !MODELED_SHEET_EXT_ELEMENTS.contains(&c.name.as_str()))
}
/// Kept sheet elements, in schema order (all after `<legacyDrawing>`, before `<tableParts>`).
const SHEET_SLOTS: &[&str] = &["legacyDrawingHF", "picture", "oleObjects", "controls"];
/// Relationship kinds Cella reads and rewrites, or that depend on content it rewrites:
/// kept content whose graph reaches one of them is dropped.
const REWRITTEN: &[&str] = &[
    "officedocument",
    "core-properties",
    "extended-properties",
    "worksheet",
    "chartsheet",
    "dialogsheet",
    "xlmacrosheet",
    "drawing",
    "chart",
    "chartex",
    "chartstyle",
    "chartcolorstyle",
    "chartusershapes",
    "table",
    "pivottable",
    "pivotcachedefinition",
    "pivotcacherecords",
    "comments",
    "vmldrawing",
    "threadedcomment",
    "person",
    "sharedstrings",
    "styles",
    "theme",
    "sheetmetadata",
    "calcchain",
    "vbaproject",
    "rdrichvalue",
    "rdrichvaluestructure",
    "rdrichvaluetypes",
    "rdrichvaluewebimage",
    "rdarray",
    "rdsupportingpropertybag",
    "rdsupportingpropertybagstructure",
    "richvaluerel",
    "powerpivotdata",
];
/// Relationship kinds a chart sheet reaches that are otherwise rewritten: its drawing, chart and
/// their style, colour and shape parts are kept byte for byte (the chart sheet is not modelled).
const CHART_SHEET_GRAPH: &[&str] = &["chartsheet", "drawing", "chart", "chartex", "chartstyle", "chartcolorstyle", "chartusershapes"];
/// Part-name prefixes (lower case) Cella's writer never produces: kept parts there keep their
/// original names. Anything else is renamed so it can't collide with written parts.
const OWN_DIRS: &[&str] = &[
    "customxml/",
    "xl/slicers/",
    "xl/slicercaches/",
    "xl/timelines/",
    "xl/timelinecaches/",
    "docprops/custom.xml",
    "customui/",
    "xl/externallinks/",
    "xl/connections.xml",
    "xl/xmlmaps.xml",
    "xl/querytables/",
    "xl/ctrlprops/",
    "xl/embeddings/",
    "xl/activex/",
    "xl/webextensions/",
    "xl/printersettings/",
    "xl/model/",
];
fn kind(rel_type: &str) -> &str {
    rel_type.rsplit('/').next().unwrap_or("")
}
fn is_kind(r: &PreservedRel, kinds: &[&str]) -> bool {
    let k = kind(&r.rel_type);
    kinds.iter().any(|x| x.eq_ignore_ascii_case(k))
}
fn rewritten(r: &PreservedRel) -> bool {
    !r.external && REWRITTEN.contains(&kind(&r.rel_type).to_ascii_lowercase().as_str())
}
pub(crate) fn rels_part_name(part: &str) -> String {
    let (dir, file) = package::split_dir(part);
    if dir.is_empty() { format!("_rels/{file}.rels") } else { format!("{dir}/_rels/{file}.rels") }
}
/// The key of a part Cella rewrites whose kept extras hang off its name (a table, PivotTable
/// or pivot cache), or `None` when the name can't be used as one.
fn owner_key(kind: &str, name: &str) -> Option<String> {
    let name = name.trim();
    if name.is_empty() || name.contains(KS) {
        return None;
    }
    Some(format!("{kind}:{}", name.to_lowercase()))
}
/// The key of a table's kept content: its name, which Excel keeps unique in a workbook.
pub fn table_key(name: &str) -> Option<String> {
    owner_key("table", name)
}
/// The key of a PivotTable's kept content (its slicer cache, its extensions).
pub fn pivot_key(name: &str) -> Option<String> {
    owner_key("pivot", name)
}
/// The key of the kept extensions of a PivotTable's cache definition.
pub fn pivot_cache_key(name: &str) -> Option<String> {
    owner_key("pivotcache", name)
}
/// The owner key of a kept relationship of a rewritten part, if it has one.
fn split_owner(id: &str) -> Option<(&str, &str)> {
    id.split_once(KS)
}
/// A kept relationship id of a rewritten part, prefixed with its owner key.
fn prefixed(key: &str, id: &str) -> String {
    format!("{key}{KS}{id}")
}
