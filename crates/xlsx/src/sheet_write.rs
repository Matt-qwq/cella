//! Writing worksheet parts and the parts they own (tables, comments, drawings, charts).

use crate::IoError;
use crate::drawing::{Obj, image_content_type, image_ext};
use crate::fmla::text_to_file;
use crate::styles::{color_attrs, dxf_id};
use crate::tables::{paper_id, px_to_col_width, px_to_pt};
use crate::write::{
    CT_CHART, CT_COMMENTS, CT_DRAWING, CT_TABLE, CT_THREADED_COMMENTS, NS_MAIN, NS_REL, NS_THREADED, Out, REL_THREADED_COMMENT, Rels, Sst, XML_DECL,
};
use crate::xml::{encode_xstring, esc, esc_attr, num};
use cella_core::{CellError, CellRef, MAX_COLS, MAX_ROWS, RangeRef, Value};
use cella_model::{
    Cell, CfOperator, CfRule, CfValueKind, Chart, Color, CondFormat, ErrorStyle, FilterCriterion, LineInfo, NoteBox, NoteCorner, Orientation,
    PageOrder, Sheet, SparklineKind, Style, StyleId, Table, TotalsFn, Validation, ValidationKind, Workbook, default_icon_thresholds, icon_count,
};
use std::collections::{BTreeMap, HashSet};
use std::fmt::Write as _;

mod parts;
mod rules;

pub(crate) use parts::{
    autofilter_xml, comments_xml, password_attrs, protected_ranges_xml, table_columns, table_name, table_xml, thread_ids, threaded_xml, vml_xml,
};
pub(crate) use rules::{cf_rule_xml, solid_bar_id, validation_xml, x14_cfvo};

fn sqref(ranges: &[RangeRef]) -> String {
    ranges.iter().map(|r| r.a1()).collect::<Vec<_>>().join(" ")
}
/// Whether a table reaches the file. A table with nothing but a header (or a header and a totals
/// row) has no data rows and is not a table the format can hold; a table slicer cache must not name
/// one that is not written.
pub(crate) fn table_is_written(t: &Table) -> bool {
    t.range.height() > t.header_row as u32 + t.totals_row as u32 || (t.header_row && t.range.height() > t.totals_row as u32)
}
/// Writes a chart's own style and colour parts beside its part (`style{n}.xml`, `colors{n}.xml`,
/// numbered like the chart part) and returns the chart part's relationships to them. `kept` holds
/// the parts the file had: a chart part that is kept or patched keeps them byte for byte, because a
/// style part describes the chart's look, not its data. A chartex part always gets both parts (Excel
/// expects them), falling back to the defaults; a classic part gets none it did not have.
fn write_style_parts(out: &mut Out, n: u32, chartex: bool, kept: (Option<&str>, Option<&str>)) -> Rels {
    let mut rels = Rels::default();
    let style = kept.0.map(str::to_string).or_else(|| chartex.then(crate::chartex::style_xml));
    let colors = kept.1.map(str::to_string).or_else(|| chartex.then(crate::chartex::colors_xml));
    if let Some(style) = style {
        out.part(&format!("xl/charts/style{n}.xml"), Some(crate::chartex::CT_STYLE), style.into_bytes());
        rels.add_uri(crate::chartex::REL_STYLE, &format!("style{n}.xml"));
    }
    if let Some(colors) = colors {
        out.part(&format!("xl/charts/colors{n}.xml"), Some(crate::chartex::CT_COLORS), colors.into_bytes());
        rels.add_uri(crate::chartex::REL_COLORS, &format!("colors{n}.xml"));
    }
    rels
}
/// Writes chart `ch`'s part (a classic `chartN.xml` or a `chartExN.xml`) and its own style and
/// colour parts, and adds the relationship that frames it in a drawing to `drawing`. Returns the
/// relationship id the frame names.
fn write_chart_part(wb: &Workbook, si: usize, ch: &Chart, out: &mut Out, drawing: &mut Rels) -> String {
    out.charts += 1;
    let n = out.charts;
    if ch.kind.is_chartex() {
        let written = crate::chartex::write_chartex(wb, si, ch, &mut out.chart_names);
        out.part(&format!("xl/charts/chartEx{n}.xml"), Some(crate::chartex::CT_CHARTEX), written.xml.into_bytes());
        // A file's own style and colour parts are kept; a regenerated part gets the defaults.
        let crels = write_style_parts(out, n, true, (written.style.as_deref(), written.colors.as_deref()));
        out.part(&format!("xl/charts/_rels/chartEx{n}.xml.rels"), None, crels.xml().into_bytes());
        drawing.add_uri(crate::chartex::REL_CHARTEX, &format!("../charts/chartEx{n}.xml"))
    } else {
        let written = crate::chart::write_chart(wb, si, ch, &out.book_name);
        out.part(&format!("xl/charts/chart{n}.xml"), Some(CT_CHART), written.xml.into_bytes());
        let crels = write_style_parts(out, n, false, (written.style.as_deref(), written.colors.as_deref()));
        if !crels.is_empty() {
            out.part(&format!("xl/charts/_rels/chart{n}.xml.rels"), None, crels.xml().into_bytes());
        }
        drawing.add("chart", &format!("../charts/chart{n}.xml"))
    }
}
/// A chart sheet part's children in schema order (ECMA-376 Part 1, §19.3): what the model keeps,
/// and the drawing that places its chart.
const CHART_SHEET_SLOTS: &[&str] = &["sheetViews", "sheetProtection", "customSheetViews", "pageMargins", "pageSetup", "headerFooter"];
/// The chart sheet's drawing: one chart frame with an absolute anchor. A chart sheet has no grid to
/// anchor to, and Excel writes an absolute anchor there; the frame is the whole window, so its size
/// only says how large the chart was laid out.
fn chart_sheet_drawing(ch: &Chart, rid: &str) -> String {
    let emu = |v: f32| (clamp_size(v.max(1.0)) as f64 * crate::tables::EMU_PER_PX).round() as i64;
    let ext = format!("<xdr:ext cx=\"{}\" cy=\"{}\"/>", emu(ch.anchor.width), emu(ch.anchor.height));
    let mut s = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<xdr:wsDr xmlns:xdr=\"http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing\" xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" xmlns:c=\"http://schemas.openxmlformats.org/drawingml/2006/chart\">",
    );
    s.push_str(&format!("<xdr:absoluteAnchor><xdr:pos x=\"0\" y=\"0\"/>{ext}"));
    if ch.kind.is_chartex() {
        let (prefix, ns) = crate::chartex::requires(ch.kind);
        s.push_str(&format!(
            "<mc:AlternateContent xmlns:mc=\"http://schemas.openxmlformats.org/markup-compatibility/2006\"><mc:Choice xmlns:{prefix}=\"{ns}\" Requires=\"{prefix}\"><xdr:graphicFrame macro=\"\"><xdr:nvGraphicFramePr><xdr:cNvPr id=\"2\" name=\"Chart 1\"{}/><xdr:cNvGraphicFramePr/></xdr:nvGraphicFramePr><a:graphic><a:graphicData uri=\"http://schemas.microsoft.com/office/drawing/2014/chartex\"><cx:chart xmlns:cx=\"http://schemas.microsoft.com/office/drawing/2014/chartex\" r:id=\"{rid}\"/></a:graphicData></a:graphic></xdr:graphicFrame></mc:Choice><mc:Fallback><xdr:sp macro=\"\" textlink=\"\"><xdr:nvSpPr><xdr:cNvPr id=\"0\" name=\"\"/><xdr:cNvSpPr><a:spLocks noTextEdit=\"1\"/></xdr:cNvSpPr></xdr:nvSpPr><xdr:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/>{ext}</a:xfrm><a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></xdr:spPr><xdr:txBody><a:bodyPr/><a:lstStyle/><a:p><a:r><a:rPr lang=\"en-US\"/><a:t>This chart type can't be shown by this application.</a:t></a:r></a:p></xdr:txBody></xdr:sp></mc:Fallback></mc:AlternateContent>",
            alt_attr(ch.alt.as_deref())
        ));
    } else {
        s.push_str(&format!(
            "<xdr:graphicFrame macro=\"\"><xdr:nvGraphicFramePr><xdr:cNvPr id=\"2\" name=\"Chart 1\"{}/><xdr:cNvGraphicFramePr/></xdr:nvGraphicFramePr><a:graphic><a:graphicData uri=\"http://schemas.openxmlformats.org/drawingml/2006/chart\"><c:chart r:id=\"{rid}\"/></a:graphicData></a:graphic></xdr:graphicFrame>",
            alt_attr(ch.alt.as_deref())
        ));
    }
    s.push_str("<xdr:clientData/></xdr:absoluteAnchor></xdr:wsDr>");
    s
}
/// The largest side of an object the writer stores, in pixels (the same bound the drawing writer
/// clamps to): anchors come from files and commands, so their numbers are hostile.
fn clamp_size(v: f32) -> f32 {
    if v.is_finite() { v.clamp(1.0, 1_000_000.0) } else { 1.0 }
}
/// A drawing object's alternative text (`xdr:cNvPr/@descr`), trimmed and capped; empty is nothing.
fn alt_attr(alt: Option<&str>) -> String {
    match alt.map(str::trim).filter(|t| !t.is_empty()) {
        Some(t) => format!(" descr=\"{}\"", esc_attr(&t.chars().take(1024).collect::<String>())),
        None => String::new(),
    }
}
/// A chart sheet part (`xl/chartsheets/sheetN.xml`): one chart, no cells. Its chart part, the
/// drawing that frames it and its kept page setup are written beside it.
///
/// Everything the model has no room for — page margins, page setup, header and footer, protection,
/// extensions — comes from the pass-through, so a chart sheet opened and saved keeps them.
pub fn write_chart_sheet(wb: &Workbook, si: usize, selected: bool, out: &mut Out) -> (String, Rels) {
    let mut rels = Rels::default();
    let Some(sheet) = wb.sheet(si) else { return (String::new(), rels) };
    let kept = crate::passthrough::SheetKept::write(sheet, &mut rels, out);
    // A chart sheet needs a view (the tab it is shown in) and page margins; where the file had its
    // own, they are kept as they were.
    let views = kept.chart_sheet_element("sheetViews").unwrap_or_else(|| {
        format!(
            "<sheetViews><sheetView{}{} workbookViewId=\"0\"/></sheetViews>",
            if selected { " tabSelected=\"1\"" } else { "" },
            if sheet.zoom == 100 { String::new() } else { format!(" zoomScale=\"{}\"", sheet.zoom.clamp(10, 400)) }
        )
    });
    let margins = kept
        .chart_sheet_element("pageMargins")
        .unwrap_or_else(|| "<pageMargins left=\"0.7\" right=\"0.7\" top=\"0.75\" bottom=\"0.75\" header=\"0.3\" footer=\"0.3\"/>".to_string());
    let mut s = format!("{XML_DECL}<chartsheet xmlns=\"{NS_MAIN}\" xmlns:r=\"{NS_REL}\">");
    // Tab colour and "fit to page" come from the model, as they do for a worksheet: the file's own
    // <sheetPr> is not kept, so changing either on a chart sheet is saved like any other edit.
    let fit = sheet.print.fit_to.is_some();
    if sheet.tab_color.is_some() || fit {
        s.push_str("<sheetPr>");
        if let Some(c) = &sheet.tab_color {
            let _ = write!(s, "<tabColor {}/>", color_attrs(c));
        }
        if fit {
            s.push_str("<pageSetUpPr fitToPage=\"1\"/>");
        }
        s.push_str("</sheetPr>");
    }
    for tag in CHART_SHEET_SLOTS {
        match *tag {
            "sheetViews" => s.push_str(&views),
            "pageMargins" => s.push_str(&margins),
            other => {
                if let Some(x) = kept.chart_sheet_element(other) {
                    s.push_str(&x);
                }
            }
        }
    }
    if let Some(ch) = sheet.chart_sheet_chart() {
        out.drawings += 1;
        let dn = out.drawings;
        let mut drels = Rels::default();
        let rid = write_chart_part(wb, si, ch, out, &mut drels);
        out.part(&format!("xl/drawings/drawing{dn}.xml"), Some(CT_DRAWING), chart_sheet_drawing(ch, &rid).into_bytes());
        if !drels.is_empty() {
            out.part(&format!("xl/drawings/_rels/drawing{dn}.xml.rels"), None, drels.xml().into_bytes());
        }
        let drid = rels.add("drawing", &format!("../drawings/drawing{dn}.xml"));
        let _ = write!(s, "<drawing r:id=\"{drid}\"/>");
    }
    for tag in ["legacyDrawing", "legacyDrawingHF", "drawingHF", "picture", "webPublishItems", "tableParts", "extLst"] {
        if let Some(x) = kept.chart_sheet_element(tag) {
            s.push_str(&x);
        }
    }
    s.push_str("</chartsheet>");
    (s, rels)
}
/// The worksheet's part and its relationships, with the sheet's rows built on at most `workers`
/// threads (see [`default_workers`]: a save passes the machine's parallelism, and the tests pass 1,
/// 2 and 8 to prove the row windows concatenate to exactly the bytes one walk writes). The only way
/// it can fail is a text cell whose string the shared-string pre-pass never saw, which would be a
/// writer bug, not a workbook one.
///
/// `name` is the part the rows belong to, and `stream` says how they reach it: streamed window by
/// window into `out` (the head before the rows are built, each window as the walk joins it, the
/// tail handed back to the caller to register the part with), or buffered into the returned
/// `String` — the whole XML — for a caller that wants the part raw. Either way the bytes are the
/// ones one buffer holds, in the same order.
pub fn write_sheet(
    wb: &Workbook,
    si: usize,
    selected: bool,
    out: &mut Out,
    slicers: &crate::slicer::SlicerWriter,
    workers: usize,
    name: &str,
    stream: bool,
) -> Result<(String, Rels), IoError> {
    let mut rels = Rels::default();
    let Some(sheet) = wb.sheet(si) else { return Ok((String::new(), rels)) };
    let styles = StyleIds { count: wb.styles.len() as u32 };

    // A table's dropdown filter is kept in `sheet.autofilter`, not in `Table::filter` (the
    // engine's only live filter state, which the reader moves a table's filter back into). Where
    // the two coincide, fold the sheet filter's criteria into the table so they reach the file:
    // the stored `colId` offsets are relative to the range's first column, so a filter that
    // starts right of the table's first column is shifted onto the table's own columns. A filter
    // covering columns outside the table has nowhere valid to go and keeps being suppressed
    // below.
    let folded: Vec<Table> = match &sheet.autofilter {
        Some(af) if !af.criteria.is_empty() => sheet
            .tables
            .iter()
            .cloned()
            .map(|mut t| {
                if t.header_row
                    && t.filter_button
                    && t.filter.is_empty()
                    && t.range.contains_range(&af.range)
                    && let Some(shift) = af.range.start.col.checked_sub(t.range.start.col)
                {
                    t.filter = af.criteria.iter().map(|(col, c)| (col.saturating_add(shift), c.clone())).collect();
                }
                t
            })
            .collect(),
        _ => sheet.tables.clone(),
    };
    // Table header cells must hold the column names.
    let mut overrides: BTreeMap<CellRef, String> = BTreeMap::new();
    let tables: Vec<(&Table, Vec<String>)> = folded.iter().filter(|t| table_is_written(t)).map(|t| (t, table_columns(t))).collect();
    for (t, cols) in &tables {
        if t.header_row {
            for (i, name) in cols.iter().enumerate() {
                overrides.insert(CellRef::new(t.range.start.row, t.range.start.col + i as u32), name.clone());
            }
        }
    }
    // Dynamic-array spills: anchor → range, and the spilled values to cache.
    let dynamic: BTreeMap<CellRef, RangeRef> = sheet
        .spill_ranges
        .iter()
        .filter(|(a, r)| !r.is_single() && sheet.cells.get(**a).is_some_and(|c| c.formula.as_ref().is_some_and(|f| f.array.is_none())))
        .map(|(a, r)| (*a, *r))
        .collect();
    let mut extra: BTreeMap<CellRef, Value> = BTreeMap::new();
    // Values inside spills and legacy array ranges. A spill's values are one array at its anchor,
    // and only the cells no stored cell already answers for are written out.
    for (a, r) in sheet.spill_ranges.iter().filter(|(_, r)| !r.is_single()) {
        let Some(array) = sheet.spill_values.get(a) else { continue };
        for c in r.iter().take(1_000_000) {
            if c == *a || sheet.cells.get(c).is_some_and(|x| x.formula.is_some() || !x.value.is_empty()) {
                continue;
            }
            let row = c.row.saturating_sub(r.start.row) as usize;
            let col = c.col.saturating_sub(r.start.col) as usize;
            if let Some(v) = array.get(row, col).cloned() {
                extra.insert(c, v);
            }
        }
    }

    let mut s = format!("{XML_DECL}<worksheet xmlns=\"{NS_MAIN}\" xmlns:r=\"{NS_REL}\">");
    // sheetPr
    let fit = sheet.print.fit_to.is_some();
    // Outline Settings: only an axis switched away from the format's default needs an attribute.
    let outline = !(sheet.summary_below && sheet.summary_right);
    if sheet.tab_color.is_some() || fit || outline {
        s.push_str("<sheetPr>");
        if let Some(c) = &sheet.tab_color {
            let _ = write!(s, "<tabColor {}/>", color_attrs(c));
        }
        if outline {
            s.push_str("<outlinePr");
            for (key, on) in [("summaryBelow", sheet.summary_below), ("summaryRight", sheet.summary_right)] {
                if !on {
                    let _ = write!(s, " {key}=\"0\"");
                }
            }
            s.push_str("/>");
        }
        if fit {
            s.push_str("<pageSetUpPr fitToPage=\"1\"/>");
        }
        s.push_str("</sheetPr>");
    }
    // `Sheet::used_range` carries the style bands (a formatted blank is a used cell), which is
    // what the dimension and the row windows need to cover (gaps §32, slice 3b).
    let mut used = sheet.used_range();
    for c in extra.keys().chain(overrides.keys()) {
        let r = RangeRef::cell(*c);
        used = Some(used.map_or(r, |u| u.union(&r)));
    }
    let _ = write!(s, "<dimension ref=\"{}\"/>", used.map(|u| u.a1()).unwrap_or_else(|| "A1".into()));

    // sheetViews
    s.push_str("<sheetViews><sheetView");
    if selected {
        s.push_str(" tabSelected=\"1\"");
    }
    if !sheet.show_gridlines {
        s.push_str(" showGridLines=\"0\"");
    }
    if !sheet.show_headings {
        s.push_str(" showRowColHeaders=\"0\"");
    }
    if sheet.show_formulas {
        s.push_str(" showFormulas=\"1\"");
    }
    if !sheet.show_zeros {
        s.push_str(" showZeros=\"0\"");
    }
    if !sheet.show_outline_symbols {
        s.push_str(" showOutlineSymbols=\"0\"");
    }
    if sheet.right_to_left {
        s.push_str(" rightToLeft=\"1\"");
    }
    if sheet.view != cella_model::SheetViewMode::Normal {
        let _ = write!(s, " view=\"{}\"", sheet.view.as_str());
    }
    if sheet.zoom != 100 && (10..=400).contains(&sheet.zoom) {
        let _ = write!(s, " zoomScale=\"{0}\" zoomScaleNormal=\"{0}\"", sheet.zoom);
    }
    if sheet.view_top_left != CellRef::default() && sheet.freeze.is_none() {
        let _ = write!(s, " topLeftCell=\"{}\"", sheet.view_top_left.a1());
    }
    s.push_str(" workbookViewId=\"0\">");
    let active = sheet.view_active.a1();
    match sheet.freeze {
        Some((rows, cols)) if rows > 0 || cols > 0 => {
            let pane = match (rows > 0, cols > 0) {
                (true, true) => "bottomRight",
                (true, false) => "bottomLeft",
                _ => "topRight",
            };
            s.push_str("<pane");
            if cols > 0 {
                let _ = write!(s, " xSplit=\"{cols}\"");
            }
            if rows > 0 {
                let _ = write!(s, " ySplit=\"{rows}\"");
            }
            let _ = write!(
                s,
                " topLeftCell=\"{}\" activePane=\"{pane}\" state=\"frozen\"/>",
                CellRef::new(
                    rows.max(sheet.view_top_left.row).min(cella_core::MAX_ROWS - 1),
                    cols.max(sheet.view_top_left.col).min(cella_core::MAX_COLS - 1)
                )
                .a1()
            );
            let _ = write!(s, "<selection pane=\"{pane}\" activeCell=\"{active}\" sqref=\"{active}\"/>");
        }
        _ => {
            let _ = write!(s, "<selection activeCell=\"{active}\" sqref=\"{active}\"/>");
        }
    }
    s.push_str("</sheetView></sheetViews>");

    // sheetFormatPr
    let max_row_outline = sheet.rows.values().map(|r| r.outline).max().unwrap_or(0);
    let max_col_outline = sheet.cols.values().map(|r| r.outline).max().unwrap_or(0);
    s.push_str("<sheetFormatPr");
    if (sheet.default_col_width - cella_model::DEFAULT_COL_WIDTH).abs() > 0.01 {
        let _ = write!(s, " defaultColWidth=\"{}\"", num(px_to_col_width(sheet.default_col_width)));
    }
    let _ = write!(s, " defaultRowHeight=\"{}\"", num(px_to_pt(sheet.default_row_height)));
    if (sheet.default_row_height - cella_model::DEFAULT_ROW_HEIGHT).abs() > 0.01 {
        s.push_str(" customHeight=\"1\"");
    }
    if max_row_outline > 0 {
        let _ = write!(s, " outlineLevelRow=\"{max_row_outline}\"");
    }
    if max_col_outline > 0 {
        let _ = write!(s, " outlineLevelCol=\"{max_col_outline}\"");
    }
    s.push_str("/>");

    // cols: runs of identical settings.
    if !sheet.cols.is_empty() {
        s.push_str("<cols>");
        let mut run: Option<(u32, u32, LineInfo)> = None;
        let flush = |s: &mut String, r: (u32, u32, LineInfo)| {
            let (a, mut b, i) = r;
            // A run that ends at the 256th column means "all remaining columns": Excel's own
            // .xls conversion writes max="16384" for the trailing style, and a partial range
            // (28..256) makes Excel treat those columns as used and grow the used range.
            if b + 1 == 256 {
                b = cella_core::MAX_COLS - 1;
            }
            let w = i.size.unwrap_or(sheet.default_col_width);
            let _ = write!(s, "<col min=\"{}\" max=\"{}\" width=\"{}\"", a + 1, b + 1, num(px_to_col_width(w)));
            if let Some(st) = i.style {
                let _ = write!(s, " style=\"{}\"", styles.get(st));
            }
            if i.hidden {
                s.push_str(" hidden=\"1\"");
            }
            if i.size.is_some() {
                s.push_str(" customWidth=\"1\"");
            }
            if i.outline > 0 {
                let _ = write!(s, " outlineLevel=\"{}\"", i.outline.min(7));
            }
            if i.collapsed {
                s.push_str(" collapsed=\"1\"");
            }
            s.push_str("/>");
        };
        for (c, info) in &sheet.cols {
            if c >= cella_core::MAX_COLS {
                continue;
            }
            run = match run {
                Some((a, b, i)) if b + 1 == c && i == *info => Some((a, c, i)),
                Some(r) => {
                    flush(&mut s, r);
                    Some((c, c, *info))
                }
                None => Some((c, c, *info)),
            };
        }
        if let Some(r) = run {
            flush(&mut s, r);
        }
        s.push_str("</cols>");
    }

    // sheetData. The store hands its cells out in (row, column) order and the sparse additions —
    // table headers, spilled values, pictures, dynamic-array anchors — are a handful, so the sheet
    // is written in one merge walk. This used to collect all 31.7M cell references into a `BTreeSet`
    // first (1.4 s and half a gigabyte on the report this was measured against) and then look every
    // cell up again, one random `cells.get` at a time.
    let mut additions: BTreeMap<u32, Vec<(u32, Additions)>> = BTreeMap::new();
    for (c, name) in &overrides {
        touch(&mut additions, *c).override_text = Some(name);
    }
    for (c, v) in &extra {
        touch(&mut additions, *c).value = Some(v);
    }
    // A cell's static picture is a rich-data value, written as the `vm` id its first use in the
    // sheet gives it. A picture sits at one cell and the rows are written ascending, so the cell
    // map's own order is the file's order; assigning the ids here leaves the row walk read-only.
    for (c, picture) in &sheet.cell_pictures {
        let vm = out.cell_pictures.index(picture);
        touch(&mut additions, *c).picture_vm = Some(vm);
    }
    for (c, r) in &dynamic {
        touch(&mut additions, *c).dynamic = Some(*r);
    }

    s.push_str("<sheetData>");
    // The head is complete: hand it to the compression thread before the rows are built, so its
    // deflate starts while the string pre-pass and the row windows run. `s` becomes the tail
    // buffer; everything below `</sheetData>` accumulates into it as before.
    if stream {
        let head = std::mem::take(&mut s);
        out.begin_stream(name, head.into_bytes())?;
    }
    // `CELLA_PROFILE` timing, read only when profiling and never on wasm (as `cmd::commit`).
    #[allow(clippy::disallowed_methods)]
    let profile = if cfg!(target_arch = "wasm32") { None } else { std::env::var_os("CELLA_PROFILE").map(|_| std::time::Instant::now()) };
    // A text cell is a reference into the workbook's shared string table, and that table's order is
    // the file's first-use order: sheet by sheet, row by row, cell by cell. Only a walk in that
    // order can establish it, so the sheet's strings are counted and given their ids first (a
    // pre-pass, `SstBatch`), and the row builder that follows only reads them (`Sst::id`).
    // That is what lets a window of rows be built without the `Out` the save is assembling.
    let windows = window_plan(sheet, &additions, workers);
    let strings = collect_strings(sheet, &additions, &windows)?;
    if let Some(t0) = profile {
        eprintln!("sheet {name}: strings {:?}", t0.elapsed());
    }
    out.sst.extend(strings.refs, &strings.strings);
    let (row_windows, flags) = build_rows(wb, si, sheet, &additions, &out.sst, styles, &windows)?;
    if let Some(t0) = profile {
        eprintln!("sheet {name}: rows {:?}", t0.elapsed());
    }
    out.needs_calc |= flags.needs_calc;
    out.dynamic_arrays |= flags.dynamic_arrays;
    if stream {
        // Each finished window goes to the compression thread as soon as the walk joins it: the
        // bytes are the ones one buffer holds, in the same order, and the deflate of the early
        // windows runs while the tail and the later parts are written. `send` holds one job, so
        // the raw XML of at most a window waits behind the one being deflated.
        for part in row_windows {
            out.stream_chunk(part.into_bytes())?;
        }
        if let Some(t0) = profile {
            eprintln!("sheet {name}: streamed {:?}", t0.elapsed());
        }
    } else {
        let capacity = row_windows.iter().map(String::len).sum();
        let mut rows = String::with_capacity(capacity);
        for part in &row_windows {
            rows.push_str(part);
        }
        s.push_str(&rows);
    }
    s.push_str("</sheetData>");

    // sheetProtection
    if let Some(p) = &sheet.protection {
        s.push_str("<sheetProtection sheet=\"1\" objects=\"1\" scenarios=\"1\"");
        for (attr, allowed) in [
            ("formatCells", p.format_cells),
            ("formatColumns", p.format_columns),
            ("formatRows", p.format_rows),
            ("insertColumns", p.insert_columns),
            ("insertRows", p.insert_rows),
            ("deleteColumns", p.delete_columns),
            ("deleteRows", p.delete_rows),
            ("sort", p.sort),
            ("autoFilter", p.autofilter),
        ] {
            if allowed {
                let _ = write!(s, " {attr}=\"0\"");
            }
        }
        if !p.select_locked {
            s.push_str(" selectLockedCells=\"1\"");
        }
        if !p.select_unlocked {
            s.push_str(" selectUnlockedCells=\"1\"");
        }
        if let Some(h) = &p.password {
            s.push_str(&password_attrs(h, ""));
        }
        s.push_str("/>");
    }
    s.push_str(&protected_ranges_xml(sheet));
    // autoFilter (a sheet filter overlapping a table would be invalid).
    if let Some(af) = &sheet.autofilter
        && !sheet.tables.iter().any(|t| t.range.intersects(&af.range))
    {
        s.push_str(&autofilter_xml(af.range, &af.criteria, out));
    }
    if !sheet.merges.is_empty() {
        let merges: Vec<&RangeRef> = sheet.merges.iter().filter(|m| !m.is_single()).collect();
        let _ = write!(s, "<mergeCells count=\"{}\">", merges.len());
        for m in merges {
            let _ = write!(s, "<mergeCell ref=\"{}\"/>", m.a1());
        }
        s.push_str("</mergeCells>");
    }
    // Conditional formats, priorities renumbered 1..n in priority order.
    let mut order: Vec<usize> = (0..sheet.cond_formats.len()).collect();
    order.sort_by_key(|&i| sheet.cond_formats.get(i).map(|c| c.priority).unwrap_or(0));
    let mut prio = vec![0u32; sheet.cond_formats.len()];
    for (rank, &i) in order.iter().enumerate() {
        if let Some(p) = prio.get_mut(i) {
            *p = rank as u32 + 1;
        }
    }
    for (i, cf) in sheet.cond_formats.iter().enumerate() {
        if cf.ranges.is_empty() {
            continue;
        }
        let _ = write!(
            s,
            "<conditionalFormatting sqref=\"{}\">{}</conditionalFormatting>",
            sqref(&cf.ranges),
            cf_rule_xml(cf, prio.get(i).copied().unwrap_or(1), out)
        );
    }
    let vals: Vec<&Validation> = sheet.validations.iter().filter(|v| !v.ranges.is_empty()).collect();
    if !vals.is_empty() {
        let _ = write!(s, "<dataValidations count=\"{}\">", vals.len());
        for v in vals {
            s.push_str(&validation_xml(v));
        }
        s.push_str("</dataValidations>");
    }
    if !sheet.hyperlinks.is_empty() {
        s.push_str("<hyperlinks>");
        for (c, h) in &sheet.hyperlinks {
            let t = h.target.trim();
            if t.is_empty() {
                continue;
            }
            // `external` is what the file said, or what Insert › Link chose; the scheme test covers
            // links made before the flag existed.
            let external = h.external
                || t.contains("://")
                || t.to_ascii_lowercase().starts_with("mailto:")
                || t.starts_with("\\\\")
                || t.to_ascii_lowercase().starts_with("file:");
            let _ = write!(s, "<hyperlink ref=\"{}\"", c.a1());
            if external {
                let (url, loc) = match t.split_once('#') {
                    Some((u, l)) if !u.is_empty() => (u, Some(l)),
                    _ => (t, None),
                };
                let id = rels.add_external("hyperlink", url);
                let _ = write!(s, " r:id=\"{id}\"");
                if let Some(l) = loc {
                    let _ = write!(s, " location=\"{}\"", esc_attr(l));
                }
            } else {
                let _ = write!(s, " location=\"{}\" display=\"{}\"", esc_attr(t.trim_start_matches('#')), esc_attr(t.trim_start_matches('#')));
            }
            if let Some(tt) = &h.tooltip {
                let _ = write!(s, " tooltip=\"{}\"", esc_attr(tt));
            }
            s.push_str("/>");
        }
        s.push_str("</hyperlinks>");
    }
    // Printing.
    let p = &sheet.print;
    if p.gridlines || p.headings || p.center_h || p.center_v {
        s.push_str("<printOptions");
        for (a, v) in [("horizontalCentered", p.center_h), ("verticalCentered", p.center_v), ("headings", p.headings), ("gridLines", p.gridlines)] {
            if v {
                let _ = write!(s, " {a}=\"1\"");
            }
        }
        s.push_str("/>");
    }
    // Unmodelled content kept from the source file (printer settings, controls, OLE objects…).
    let kept = crate::passthrough::SheetKept::write(sheet, &mut rels, out);
    let m = |i: usize| num(p.margins.get(i).copied().unwrap_or(0.75) as f64);
    let _ =
        write!(s, "<pageMargins left=\"{}\" right=\"{}\" top=\"{}\" bottom=\"{}\" header=\"{}\" footer=\"{}\"/>", m(0), m(1), m(2), m(3), m(4), m(5));
    s.push_str("<pageSetup");
    if let Some(id) = paper_id(&p.paper)
        && id != 1
    {
        let _ = write!(s, " paperSize=\"{id}\"");
    }
    if p.scale != 100 && (10..=400).contains(&p.scale) {
        let _ = write!(s, " scale=\"{}\"", p.scale);
    }
    if let Some((w, h)) = p.fit_to {
        let _ = write!(s, " fitToWidth=\"{w}\" fitToHeight=\"{h}\"");
    }
    let _ = write!(s, " orientation=\"{}\"", if p.orientation == Orientation::Landscape { "landscape" } else { "portrait" });
    // Excel writes the default order only when it is the file's odd one out; both values are
    // legal, so the default stays out of the file.
    if p.page_order != PageOrder::DownThenOver {
        let _ = write!(s, " pageOrder=\"{}\"", p.page_order.id());
    }
    if p.black_and_white {
        s.push_str(" blackAndWhite=\"1\"");
    }
    if p.draft {
        s.push_str(" draft=\"1\"");
    }
    if let Some(id) = kept.page_setup_id() {
        let _ = write!(s, " r:id=\"{id}\"");
    }
    s.push_str("/>");
    if p.has_header_footer() || p.different_first || p.different_odd_even || !p.hf_scale_with_doc || !p.hf_align_with_margins {
        s.push_str("<headerFooter");
        for (attr, on, default) in [
            ("differentOddEven", p.different_odd_even, false),
            ("differentFirst", p.different_first, false),
            ("scaleWithDoc", p.hf_scale_with_doc, true),
            ("alignWithMargins", p.hf_align_with_margins, true),
        ] {
            if on != default {
                let _ = write!(s, " {attr}=\"{}\"", u8::from(on));
            }
        }
        s.push('>');
        // CT_HeaderFooter's element order.
        for (tag, text) in [
            ("oddHeader", &p.header),
            ("oddFooter", &p.footer),
            ("evenHeader", &p.even_header),
            ("evenFooter", &p.even_footer),
            ("firstHeader", &p.first_header),
            ("firstFooter", &p.first_footer),
        ] {
            if !text.is_empty() {
                let _ = write!(s, "<{tag}>{}</{tag}>", esc(text));
            }
        }
        s.push_str("</headerFooter>");
    }
    for (tag, list, max) in [("rowBreaks", &p.row_breaks, 16383u32), ("colBreaks", &p.col_breaks, 1048575)] {
        let list: Vec<u32> = list.iter().copied().filter(|&b| b > 0).collect();
        if !list.is_empty() {
            let _ = write!(s, "<{tag} count=\"{0}\" manualBreakCount=\"{0}\">", list.len());
            for b in list {
                let _ = write!(s, "<brk id=\"{b}\" max=\"{max}\" man=\"1\"/>");
            }
            let _ = write!(s, "</{tag}>");
        }
    }

    // Drawing: pictures, charts, shapes, equations, and the frames that place slicers and timelines.
    let frames = slicers.frames(si);
    if !sheet.images.is_empty()
        || !sheet.charts.is_empty()
        || !sheet.kept_charts.is_empty()
        || !sheet.shapes.is_empty()
        || !sheet.equations.is_empty()
        || !frames.is_empty()
    {
        out.drawings += 1;
        let dn = out.drawings;
        let mut drels = Rels::default();
        let mut objs: Vec<Obj<'_>> = vec![];
        for img in &sheet.images {
            if img.data.is_empty() {
                continue;
            }
            let ext = image_ext(&img.mime, &img.data);
            out.default_type(ext, image_content_type(ext));
            let file = out.image_part(ext, &img.data);
            let rid = drels.add("image", &format!("../media/{file}"));
            objs.push(Obj::Image(img, rid));
        }
        for ch in &sheet.charts {
            let rid = write_chart_part(wb, si, ch, out, &mut drels);
            objs.push(Obj::Chart(ch, rid));
        }
        for ch in &sheet.kept_charts {
            out.charts += 1;
            let n = out.charts;
            let rid = if ch.chartex {
                out.part(&format!("xl/charts/chartEx{n}.xml"), Some(crate::chartex::CT_CHARTEX), ch.xml.as_bytes().to_vec());
                // The part is kept whole, so its look goes with it.
                let crels = write_style_parts(out, n, true, (ch.style.as_deref(), ch.colors.as_deref()));
                out.part(&format!("xl/charts/_rels/chartEx{n}.xml.rels"), None, crels.xml().into_bytes());
                drels.add_uri(crate::chartex::REL_CHARTEX, &format!("../charts/chartEx{n}.xml"))
            } else {
                // Byte for byte as read: Cella does not model this chart's kind.
                out.part(&format!("xl/charts/chart{n}.xml"), Some(CT_CHART), ch.xml.as_bytes().to_vec());
                let crels = write_style_parts(out, n, false, (ch.style.as_deref(), ch.colors.as_deref()));
                if !crels.is_empty() {
                    out.part(&format!("xl/charts/_rels/chart{n}.xml.rels"), None, crels.xml().into_bytes());
                }
                drels.add("chart", &format!("../charts/chart{n}.xml"))
            };
            objs.push(Obj::RawChart(ch, rid));
        }
        for sh in &sheet.shapes {
            objs.push(Obj::Shape(sh));
        }
        for eq in &sheet.equations {
            objs.push(Obj::Equation(eq));
        }
        for f in &frames {
            objs.push(Obj::Widget { name: f.name, anchor: f.anchor, timeline: f.timeline });
        }
        out.part(&format!("xl/drawings/drawing{dn}.xml"), Some(CT_DRAWING), crate::drawing::write_drawing(sheet, &wb.theme, &objs).into_bytes());
        if !drels.is_empty() {
            out.part(&format!("xl/drawings/_rels/drawing{dn}.xml.rels"), None, drels.xml().into_bytes());
        }
        let rid = rels.add("drawing", &format!("../drawings/drawing{dn}.xml"));
        let _ = write!(s, "<drawing r:id=\"{rid}\"/>");
    }
    // Notes and form controls share one legacy VML drawing: notes need it for Excel to show them,
    // and controls carry their `x:ClientData` there. Shape ids come from 1024-id blocks listed in
    // the drawing's idmap (after the blocks kept header and footer drawings use), notes first, so
    // a control's `shapeId` names its VML shape. Threaded comments also get a legacy note (its
    // text with the replies as lines, for readers without threaded comments) linked by id.
    let mut controls = String::new();
    if !sheet.comments.is_empty() || !sheet.controls.is_empty() {
        let shapes = (sheet.comments.len() as u64).saturating_add(sheet.controls.len() as u64);
        let blocks = u32::try_from(shapes.saturating_add(1).div_ceil(1024)).unwrap_or(u32::MAX).max(1);
        let first = out.kept.notes_block(out.vml_block.saturating_add(1));
        out.vml_block = out.vml_block.saturating_add(blocks);
        let idmap = (first..first.saturating_add(blocks)).map(|b| b.to_string()).collect::<Vec<_>>().join(",");
        // With a kept drawing, our shapes join it in a block of their own.
        let base = kept.shape_base().unwrap_or(u64::from(first) * 1024);
        let notes = sheet.comments.len() as u64;
        let mut control_shapes = String::new();
        if !sheet.controls.is_empty() {
            let w = crate::controls::write(out, &mut rels, sheet, |i| base + 1 + notes + i as u64);
            controls = w.entries;
            control_shapes = format!("{}{}", crate::controls::SHAPETYPE, w.vml);
        }
        if !sheet.comments.is_empty() {
            out.comments += 1;
            let n = out.comments;
            let ids = thread_ids(sheet, si);
            out.part(&format!("xl/comments{n}.xml"), Some(CT_COMMENTS), comments_xml(sheet, &ids).into_bytes());
            if !ids.is_empty() {
                let x = threaded_xml(sheet, &ids, &mut out.persons);
                out.part(&format!("xl/threadedComments/threadedComment{n}.xml"), Some(CT_THREADED_COMMENTS), x.into_bytes());
                rels.add_uri(REL_THREADED_COMMENT, &format!("../threadedComments/threadedComment{n}.xml"));
            }
            rels.add("comments", &format!("../comments{n}.xml"));
        }
        out.vmls += 1;
        let v = out.vmls;
        out.default_type("vml", "application/vnd.openxmlformats-officedocument.vmlDrawing");
        let vml_name = format!("xl/drawings/vmlDrawing{v}.vml");
        let vml = kept.notes_vml(vml_xml(sheet, &idmap, base, &control_shapes), &vml_name, out);
        out.part(&vml_name, None, vml.into_bytes());
        let rid = rels.add("vmlDrawing", &format!("../drawings/vmlDrawing{v}.vml"));
        let _ = write!(s, "<legacyDrawing r:id=\"{rid}\"/>");
    } else if let Some(rid) = kept.lone_vml(&mut rels, out) {
        let _ = write!(s, "<legacyDrawing r:id=\"{rid}\"/>");
    }
    s.push_str(&kept.elements(&controls));
    // Tables.
    if !tables.is_empty() {
        let mut parts = String::new();
        let mut used_names: HashSet<String> = HashSet::new();
        for (t, cols) in &tables {
            out.tables += 1;
            out.table_id += 1;
            let n = out.tables;
            let name = table_name(&t.name, &mut used_names, out.table_id);
            // A table slicer cache names its table by this id; record it under the model's name (the
            // slicer's `table`), which is what the slicer model holds.
            out.table_ids.push((si, t.name.clone(), out.table_id));
            // What the table owned in the source file (its query table, its extensions) hangs off
            // its name: a renamed or deleted table takes it with it.
            let mut trels = Rels::default();
            let key = crate::passthrough::table_key(&name);
            let ext = crate::passthrough::owner_extras(sheet.preserved.as_deref(), key.as_deref(), &mut trels, out);
            let xml = table_xml(t, cols, &name, out.table_id, out, &ext);
            out.part(&format!("xl/tables/table{n}.xml"), Some(CT_TABLE), xml.into_bytes());
            if !trels.is_empty() {
                out.part(&format!("xl/tables/_rels/table{n}.xml.rels"), None, trels.xml().into_bytes());
            }
            let rid = rels.add("table", &format!("../tables/table{n}.xml"));
            let _ = write!(parts, "<tablePart r:id=\"{rid}\"/>");
        }
        let _ = write!(s, "<tableParts count=\"{}\">{parts}</tableParts>", tables.len());
    }
    // Excel 2010 extensions: solid data bars, then sparklines, then the ones Cella does not
    // model (timelines, filters…) as they came.
    let solid_bars: Vec<(&CondFormat, u32)> = sheet
        .cond_formats
        .iter()
        .enumerate()
        .filter(|(_, cf)| !cf.ranges.is_empty() && matches!(cf.rule, CfRule::DataBar { gradient: false, .. }))
        .map(|(i, cf)| (cf, prio.get(i).copied().unwrap_or(1)))
        .collect();
    // The modelled timelines of this sheet, with their own relationships (written from the model;
    // the kept copy of this extension was stripped on read).
    let model_exts = slicers.sheet_ext(si, &mut rels);
    let kept_exts = kept.ext_elements();
    if !solid_bars.is_empty() || !sheet.sparklines.is_empty() || !kept_exts.is_empty() || !model_exts.is_empty() {
        s.push_str("<extLst>");
    }
    if !solid_bars.is_empty() {
        let _ = write!(
            s,
            "<ext uri=\"{{78C0D931-6437-407d-A8A1-0F90B4350D7D}}\" xmlns:x14=\"{X14_NS}\" xmlns:xm=\"{XM_NS}\"><x14:conditionalFormattings>"
        );
        for (cf, p) in &solid_bars {
            if let CfRule::DataBar { min, max, .. } = &cf.rule {
                let (id, lo, hi) = (solid_bar_id(*p), x14_cfvo(min), x14_cfvo(max));
                let _ = write!(s, "<x14:conditionalFormatting><x14:cfRule type=\"dataBar\" id=\"{id}\"><x14:dataBar gradient=\"0\">{lo}{hi}");
                let _ = write!(s, "</x14:dataBar></x14:cfRule><xm:sqref>{}</xm:sqref></x14:conditionalFormatting>", sqref(&cf.ranges));
            }
        }
        s.push_str("</x14:conditionalFormattings></ext>");
    }
    if !sheet.sparklines.is_empty() {
        let _ = write!(s, "<ext uri=\"{{05C60535-1F16-4fd2-B633-F4F36F0B64E0}}\" xmlns:x14=\"{X14_NS}\"><x14:sparklineGroups xmlns:xm=\"{XM_NS}\">");
        for sp in &sheet.sparklines {
            let ty = match sp.kind {
                SparklineKind::Line => "",
                SparklineKind::Column => " type=\"column\"",
                SparklineKind::WinLoss => " type=\"stacked\"",
            };
            let src = if sp.source.contains('!') { sp.source.clone() } else { format!("{}!{}", cella_formula::quote_sheet(&sheet.name), sp.source) };
            let color = if sp.color == Color::Auto { "rgb=\"FF376092\"".to_string() } else { color_attrs(&sp.color) };
            let _ = write!(
                s,
                "<x14:sparklineGroup displayEmptyCellsAs=\"gap\"{ty}{}><x14:colorSeries {color}/><x14:sparklines><x14:sparkline><xm:f>{}</xm:f><xm:sqref>{}</xm:sqref></x14:sparkline></x14:sparklines></x14:sparklineGroup>",
                if sp.markers { " markers=\"1\"" } else { "" },
                esc(&src),
                sp.cell.a1()
            );
        }
        s.push_str("</x14:sparklineGroups></ext>");
    }
    if !solid_bars.is_empty() || !sheet.sparklines.is_empty() || !kept_exts.is_empty() || !model_exts.is_empty() {
        s.push_str(&model_exts);
        s.push_str(&kept_exts);
        s.push_str("</extLst>");
    }
    s.push_str("</worksheet>");
    Ok((s, rels))
}
/// Style ids as the file spells them: a style the workbook's table does not hold is written as 0
/// (a reader can hand out ids the writer's own table lacks).
#[derive(Clone, Copy)]
struct StyleIds {
    count: u32,
}

impl StyleIds {
    fn get(self, s: cella_model::StyleId) -> u32 {
        if s.0 < self.count { s.0 } else { 0 }
    }
}

/// What the row walk reports to the workbook assembly: a formula with no cached value makes Excel
/// recalculate, and a formula that needs array evaluation sets the cell metadata flag.
#[derive(Clone, Copy, Default)]
struct RowFlags {
    needs_calc: bool,
    dynamic_arrays: bool,
}

/// Drops one row from inclusive spans, splitting the span that holds it, as the model's
/// `Sheet::filter_row_ranges` drops a table's totals row.
fn drop_row(spans: &mut Vec<(u32, u32)>, row: u32) {
    let mut out = Vec::with_capacity(spans.len().saturating_add(1));
    for (a, b) in spans.drain(..) {
        if row < a || row > b {
            out.push((a, b));
            continue;
        }
        if a < row {
            out.push((a, row.saturating_sub(1)));
        }
        if row < b {
            out.push((row.saturating_add(1), b));
        }
    }
    *spans = out;
}

/// The rows a save's filters can be read back onto: the ranges `Sheet::filter_row_ranges` names
/// once the file is opened again. The sheet's AutoFilter reaches the file only when no table
/// intersects it — a table containing its whole range folds its criteria into the table part,
/// which reading promotes back to the sheet's filter — and each table's own filter travels in its
/// part. A sheet filter that only partly overlaps a table is suppressed (there is no element for
/// it), so a row it hides is not written as hidden: reading the row back would keep it
/// hand-hidden, a state the model never held and Clear Filter cannot clear ("rows differ after
/// reload", found by the command-sequence fuzzer: a sheet copied onto itself, a filter, a table
/// inside its range, an autofill, a three-step undo and a second filter).
///
/// `None` when no AutoFilter with criteria is live, and every row the bitmap hides is written
/// as it always was: a workbook built by hand can hide rows in the bitmap with no filter anywhere
/// to own them.
fn written_filter_ranges(sheet: &Sheet) -> Option<Vec<(u32, u32)>> {
    let af = sheet.autofilter.as_ref()?;
    if af.criteria.is_empty() {
        return None;
    }
    let mut out = Vec::new();
    // The fold's own condition: a table that reaches the file, with its filter button, whose range
    // contains the filter's whole range.
    let folded = sheet
        .tables
        .iter()
        .any(|t| table_is_written(t) && t.header_row && t.filter_button && t.filter.is_empty() && t.range.contains_range(&af.range));
    if folded || !sheet.tables.iter().any(|t| t.range.intersects(&af.range)) {
        let first = af.range.start.row.saturating_add(1);
        if first <= af.range.end.row {
            out.push((first, af.range.end.row));
        }
    }
    for t in &sheet.tables {
        if table_is_written(t)
            && !t.filter.is_empty()
            && let Some(d) = t.data_range()
            && d.start.row <= d.end.row
        {
            out.push((d.start.row, d.end.row));
        }
        // A totals row is never a filter's, whatever range the sheet's AutoFilter spells.
        if t.totals_row && t.range.end.row > t.range.start.row {
            drop_row(&mut out, t.range.end.row);
        }
    }
    Some(out)
}

/// One pass over a window of a sheet's rows in the order the file writes them: the store's cells
/// merged with the sparse additions, the sheet's own line info, and the rows a filter hides. The
/// shared-string pre-pass and the row builder are both this walk, so the table one fills is exactly
/// the table the other reads, cell for cell and in the same order.
trait RowVisitor {
    /// `<row …`, up to the point where the cells (or the closing `/>`) follow.
    fn row_open(&mut self, row: u32, info: &LineInfo);
    /// A row with nothing to write: `/>`.
    fn row_empty(&mut self);
    /// The `>` that opens a row's cells.
    fn row_cells(&mut self);
    /// `</row>`.
    fn row_close(&mut self);
    /// One cell of the row being written.
    fn cell(&mut self, at: CellRef, cell: Option<&Cell>, additions: &Additions<'_>) -> Result<(), IoError>;
}

/// Walks the rows of `window` (whole rows, ascending) and hands each row and cell to `v`.
fn walk_rows<V: RowVisitor>(sheet: &Sheet, window: RangeRef, additions: &BTreeMap<u32, Vec<(u32, Additions<'_>)>>, v: &mut V) -> Result<(), IoError> {
    // A save walks every cell once, so it scans without filling the page cache: the viewport's
    // working set stays where it was, and the sheet's tail is not left resident.
    let mut store = sheet.cells.iter_scan(window).peekable();
    let mut sparse = additions.range(window.start.row..=window.end.row).peekable();
    let mut info_rows = sheet.rows.range(window.start.row..=window.end.row).peekable();
    // The rows a style band covers are written even when nothing is stored there: each of their
    // columns is a styled blank cell (`RowBuilder` emits them; gaps §32, slice 3b).
    let mut band_rows = band_rows(sheet, window).peekable();
    // Only a row the file's filters can own is written as one a filter hides; see
    // `written_filter_ranges`.
    let attributable = written_filter_ranges(sheet);
    let mut filter_rows = sheet
        .filter_hidden
        .rows()
        .skip_while(|r| *r < window.start.row)
        .take_while(|r| *r <= window.end.row)
        .filter(|r| attributable.as_ref().is_none_or(|a| a.iter().any(|&(x, y)| *r >= x && *r <= y)))
        .peekable();
    loop {
        let next_store = store.peek().map(|(at, _)| at.row);
        let next_sparse = sparse.peek().map(|(r, _)| **r);
        let next_info = info_rows.peek().map(|(r, _)| *r);
        let next_filter = filter_rows.peek().copied();
        let next_band = band_rows.peek().copied();
        let Some(row) = [next_store, next_sparse, next_info, next_filter, next_band].into_iter().flatten().min() else { break };
        let has_band = next_band == Some(row);
        if has_band {
            band_rows.next();
        }
        let mut info = if next_info == Some(row) {
            info_rows.next().map(|(_, i)| *i).unwrap_or_default()
        } else {
            sheet.rows.get(&row).copied().unwrap_or_default()
        };
        // A row the filter hides lives in the bitmap, not in the line map.
        if next_filter == Some(row) {
            filter_rows.next();
            info.hidden = true;
        }
        let mut adds: std::iter::Peekable<std::vec::IntoIter<(u32, Additions<'_>)>> = if next_sparse == Some(row) {
            sparse.next().map(|(_, v)| v.clone()).unwrap_or_default().into_iter().peekable()
        } else {
            Vec::new().into_iter().peekable()
        };
        let has_cells = next_store == Some(row) || adds.peek().is_some() || has_band;
        if !has_cells && info == LineInfo::default() {
            continue;
        }
        v.row_open(row, &info);
        if !has_cells {
            v.row_empty();
            continue;
        }
        v.row_cells();
        loop {
            let stored = store.peek().filter(|(at, _)| at.row == row);
            let add_col = adds.peek().map(|(col, _)| *col);
            let mut a = Additions::default();
            let (col, cell) = match (stored, add_col) {
                (Some((at, _)), Some(acol)) if acol < at.col => {
                    let (col, addition) = adds.next().unwrap_or((0, Additions::default()));
                    a = addition;
                    (col, None)
                }
                (Some(_), _) => {
                    let (at, cell) = store.next().unwrap_or((CellRef::new(row, 0), Cell::default()));
                    if add_col == Some(at.col) {
                        a = adds.next().map(|(_, addition)| addition).unwrap_or_default();
                    }
                    (at.col, Some(cell))
                }
                (None, Some(_)) => {
                    let (col, addition) = adds.next().unwrap_or((0, Additions::default()));
                    a = addition;
                    (col, None)
                }
                (None, None) => break,
            };
            v.cell(CellRef::new(row, col), cell.as_ref(), &a)?;
        }
        v.row_close();
    }
    Ok(())
}

/// The rows of `window` that a style band covers, ascending without repeats: a band writes a styled
/// blank cell for every column it covers on every row of itself, so those rows must be visited even
/// when the store holds nothing in them (gaps §32, slice 3b).
fn band_rows(sheet: &Sheet, window: RangeRef) -> impl Iterator<Item = u32> + '_ {
    let mut runs: Vec<(u32, u32)> =
        sheet.style_bands.iter().map(|b| (b.range.start.row.max(window.start.row), b.range.end.row.min(window.end.row))).collect();
    runs.retain(|(a, b)| a <= b);
    runs.sort_unstable();
    let mut merged: Vec<(u32, u32)> = Vec::with_capacity(runs.len());
    for (a, b) in runs {
        match merged.last_mut() {
            Some(last) if a <= last.1.saturating_add(1) => last.1 = last.1.max(b),
            _ => merged.push((a, b)),
        }
    }
    merged.into_iter().flat_map(|(a, b)| a..=b)
}

/// The texts one window of a sheet writes: its distinct strings in first-use order, the strings
/// already seen in the window, and how many text cells it wrote (a repeat still counts as a
/// reference). This is the pre-pass a save needs because the shared string table's order is the
/// file's order, and a row window cannot know what the windows before it put in the table.
#[derive(Default)]
struct SstBatch {
    strings: Vec<std::sync::Arc<str>>,
    seen: HashSet<std::sync::Arc<str>>,
    refs: u64,
}

impl SstBatch {
    /// Appends `other`'s strings after this batch's, keeping first-use order across the windows,
    /// and adds up the references they stand for.
    #[cfg(not(target_arch = "wasm32"))]
    fn append(&mut self, other: SstBatch) {
        self.refs = self.refs.saturating_add(other.refs);
        self.strings.extend(other.strings);
    }

    /// A text counted from a borrowed string: the table owns one `Arc` of it (a cell's own, when
    /// there is one, comes through `text_value`). A repeat costs a hash, not an allocation.
    fn text(&mut self, t: &str) {
        self.refs = self.refs.saturating_add(1);
        if !self.seen.contains(t) {
            let owned: std::sync::Arc<str> = std::sync::Arc::from(t);
            if self.seen.insert(std::sync::Arc::clone(&owned)) {
                self.strings.push(owned);
            }
        }
    }

    /// A text the value being written already owns: the `Arc` is kept, not copied.
    fn text_value(&mut self, t: &std::sync::Arc<str>) {
        self.refs = self.refs.saturating_add(1);
        if self.seen.insert(std::sync::Arc::clone(t)) {
            self.strings.push(std::sync::Arc::clone(t));
        }
    }
}

/// The pre-pass as the row walk sees it — every cell rebuilt. A paged store's only path (see
/// [`collect_window`]), and the reference the parity test holds the packed collector to: on a
/// live-band sheet the two must answer exactly the same `(refs, strings)`, cell for cell.
impl RowVisitor for SstBatch {
    fn row_open(&mut self, _row: u32, _info: &LineInfo) {}
    fn row_empty(&mut self) {}
    fn row_cells(&mut self) {}
    fn row_close(&mut self) {}
    fn cell(&mut self, _at: CellRef, cell: Option<&Cell>, a: &Additions<'_>) -> Result<(), IoError> {
        // A picture cell caches `#VALUE!`, and a formula's text result goes into the cell itself
        // (`t="str"`), so neither reaches the shared strings.
        if a.picture_vm.is_some() || formula_of(cell, a).is_some() {
            return Ok(());
        }
        if let Value::Text(t) = cell_value(cell, a.override_text, a.value) {
            self.text_value(&t);
        }
        Ok(())
    }
}

/// The pre-pass as the row walk sees it (see the `RowVisitor` above): a paged store's collector,
/// and the reference the parity test runs against the packed one.
fn collect_window_cells(sheet: &Sheet, additions: &BTreeMap<u32, Vec<(u32, Additions<'_>)>>, rows: (u32, u32)) -> Result<SstBatch, IoError> {
    let mut batch = SstBatch::default();
    walk_rows(sheet, RangeRef::rows(rows.0, rows.1), additions, &mut batch)?;
    Ok(batch)
}

/// The text a value writes after `cell_value`'s normalization: an array's first element, and a
/// text — the errors a file cannot hold are coerced to a number and an error, never to a text.
fn text_of(v: &Value) -> Option<&std::sync::Arc<str>> {
    match v {
        Value::Array(a) => match a.get(0, 0) {
            Some(Value::Text(t)) => Some(t),
            _ => None,
        },
        Value::Text(t) => Some(t),
        _ => None,
    }
}

/// One cell's contribution to the shared string pre-pass, on the packed value the store hands over.
/// The resolution is the one `cell_value` gives the row builder: an override replaces the cell
/// (formula and all, so `formula_of` answers none), an extra value is the cell's own, and the
/// stored value is what writes. The order is the visitor's with one liberty: an extra never sits
/// on a formula cell (the spill collector refuses one), so taking the extra before deciding
/// whether a packed `None` is a formula matches what the visitor sees on every sheet a save
/// builds.
fn count_packed(batch: &mut SstBatch, sheet: &Sheet, at: CellRef, packed: Option<cella_model::Packed<'_>>, a: &Additions<'_>) {
    if a.picture_vm.is_some() {
        return;
    }
    if let Some(text) = a.override_text {
        batch.text(text.as_str());
        return;
    }
    if let Some(value) = a.value {
        if let Some(t) = text_of(value) {
            batch.text_value(t);
        }
        return;
    }
    match packed {
        // A stored text is the cell's own value; the other packed kinds are never a text.
        Some(cella_model::Packed::Text(t)) => batch.text(t),
        Some(_) => {}
        None => {
            // A packed `None` is a cell whose value the evaluator owns. A formula writes its
            // result inline (`t="str"`), never into the shared strings — one cheap formula read
            // says so, without rebuilding the cell. The rare cell that is neither a formula nor a
            // blank is a stored array, whose first element is the value the cell writes
            // (`cell_value` resolves it for the row builder, so the pre-pass must too): one
            // rebuilt cell answers it.
            if sheet.cells.formula(at).is_some() {
                return;
            }
            if let Some(cell) = sheet.cells.get(at)
                && let Some(t) = text_of(&cell.value)
            {
                batch.text_value(t);
            }
        }
    }
}

/// One window's shared strings, collected on this thread. A paged store falls back to the cell
/// walk ([`collect_window_cells`]): the packed read resolves a paged band through the page cache —
/// filling and evicting it — where the save's own scan decodes one band at a time without touching
/// the cache, and `a_paged_sheet_saves_to_the_same_bytes` holds a save to exactly that. On the live
/// bands a normal save holds, the walk is the store's packed one: a cell's value arrives as the
/// page holds it, with `None` for the cells whose value the evaluator owns, and no `Cell` is
/// rebuilt for a cell that only needed its value — a formula's `Arc` bump and a number's payload
/// decode were most of what the pre-pass used to pay. The additions are merged in the walk's
/// order, so the table this fills is the table the row builder reads, cell for cell and in the
/// same order.
fn collect_window(sheet: &Sheet, additions: &BTreeMap<u32, Vec<(u32, Additions<'_>)>>, rows: (u32, u32)) -> Result<SstBatch, IoError> {
    if sheet.cells.paged_band_count() > 0 {
        return collect_window_cells(sheet, additions, rows);
    }
    let mut batch = SstBatch::default();
    let mut rest = additions.range(rows.0..=rows.1).peekable();
    // The additions of the row the walk is in, at or after the last stored column: the ones
    // before it were counted already.
    let mut open: Option<(u32, std::iter::Peekable<std::vec::IntoIter<(u32, Additions<'_>)>>)> = None;
    let _ = sheet.cells.visit_packed(RangeRef::rows(rows.0, rows.1), &mut |at, packed| {
        // Additions strictly before this cell: whatever the row already open holds left of its
        // column (all of it, if the walk has left that row), then whole addition rows above this
        // cell's, then this row's columns below the stored one.
        if let Some((row, mut cells)) = open.take() {
            if row < at.row {
                for (col, a) in cells {
                    count_packed(&mut batch, sheet, CellRef::new(row, col), None, &a);
                }
            } else {
                while cells.peek().is_some_and(|(col, _)| *col < at.col) {
                    let (col, a) = cells.next().unwrap_or((0, Additions::default()));
                    count_packed(&mut batch, sheet, CellRef::new(row, col), None, &a);
                }
                open = Some((row, cells));
            }
        }
        while open.is_none() {
            let Some(entry) = rest.peek() else { break };
            let row = *entry.0;
            if row > at.row {
                break;
            }
            let cells = rest.next().map_or(Vec::new(), |(_, v)| v.clone());
            let mut cells = cells.into_iter().peekable();
            while let Some(&(col, _)) = cells.peek() {
                if row == at.row && col >= at.col {
                    break;
                }
                let (col, a) = cells.next().unwrap_or((0, Additions::default()));
                count_packed(&mut batch, sheet, CellRef::new(row, col), None, &a);
            }
            if row == at.row {
                open = Some((row, cells));
                break;
            }
        }
        // The addition at this cell's column, if the sheet put one there.
        let a = open
            .as_mut()
            .and_then(|(row, cells)| {
                (*row == at.row && cells.peek().is_some_and(|(col, _)| *col == at.col)).then(|| cells.next().map(|(_, a)| a)).flatten()
            })
            .unwrap_or_default();
        count_packed(&mut batch, sheet, at, packed, &a);
        true
    });
    // What the additions still hold, in the walk's order: the tail of the last row the walk was
    // in, then the addition rows below the last stored cell — a spilled value under the data, a
    // table header the store does not hold.
    if let Some((row, cells)) = open.take() {
        for (col, a) in cells {
            count_packed(&mut batch, sheet, CellRef::new(row, col), None, &a);
        }
    }
    while let Some(entry) = rest.peek() {
        let row = *entry.0;
        let cells = rest.next().map_or(Vec::new(), |(_, v)| v.clone());
        for (col, a) in cells {
            count_packed(&mut batch, sheet, CellRef::new(row, col), None, &a);
        }
    }
    Ok(batch)
}

/// One column run a style band puts a style on for the row being written.
#[derive(Clone, Copy, Debug)]
struct BandRun {
    next: u32,
    last: u32,
    style: StyleId,
}

/// Builds one window of a sheet's rows: the `<row>` elements into `xml` and the flags the workbook
/// assembly reads back. It touches no [`Out`], which is what lets a window be built on a worker
/// thread.
struct RowBuilder<'a> {
    wb: &'a Workbook,
    si: usize,
    styles: StyleIds,
    sst: &'a Sst,
    xml: String,
    flags: RowFlags,
    /// The band columns of the row being written, ascending, overlaps already resolved (the newest
    /// band wins). A styled blank is a `<c>` element and nothing else, so it is emitted here while
    /// the row is built — the file has no range style to carry it (gaps §32, slice 3b).
    band_runs: Vec<BandRun>,
    band_at: usize,
    band_row: u32,
}

impl<'a> RowBuilder<'a> {
    fn new(wb: &'a Workbook, si: usize, styles: StyleIds, sst: &'a Sst) -> RowBuilder<'a> {
        RowBuilder { wb, si, styles, sst, xml: String::new(), flags: RowFlags::default(), band_runs: Vec::new(), band_at: 0, band_row: 0 }
    }
}

impl RowBuilder<'_> {
    fn push_cell(&mut self, at: CellRef, cell: Option<&Cell>, a: &Additions<'_>) -> Result<(), IoError> {
        // A non-spilling formula may still need array evaluation to avoid implicit intersection.
        // The analysis reads the cell's own formula: a table header's override writes its text and
        // no formula at all.
        let array = a.dynamic.or_else(|| {
            let f = cell?.formula.as_ref()?;
            (f.array.is_none() && crate::array_formula::needs_array(self.wb, self.si, at, f)).then_some(RangeRef::cell(at))
        });
        write_cell(&mut self.xml, self.styles, self.sst, at, cell, a, array.as_ref(), &mut self.flags)
    }

    /// The band columns of `row`, ascending and merged: the file has no range style, so the
    /// rectangle becomes one styled blank a column. Where bands overlap, the newest wins (a later
    /// format was applied on top), and runs of one style collapse into one entry.
    fn band_runs_for(&self, row: u32) -> Vec<BandRun> {
        let Some(sh) = self.wb.sheet(self.si) else { return Vec::new() };
        if sh.style_bands.is_empty() {
            return Vec::new();
        }
        let mut hits: Vec<(usize, &cella_model::StyleBand)> =
            sh.style_bands.iter().enumerate().filter(|(_, b)| row >= b.range.start.row && row <= b.range.end.row).collect();
        if hits.is_empty() {
            return Vec::new();
        }
        hits.sort_by_key(|(_, b)| b.range.start.col);
        let mut edges: Vec<u32> = Vec::with_capacity(hits.len() * 2);
        for (_, b) in &hits {
            edges.push(b.range.start.col);
            edges.push(b.range.end.col.saturating_add(1));
        }
        edges.sort_unstable();
        edges.dedup();
        let mut runs: Vec<BandRun> = Vec::with_capacity(edges.len());
        for window in edges.windows(2) {
            let (lo, hi) = (window[0], window[1]);
            if lo >= hi {
                continue;
            }
            // The last band of the sheet covering this piece is the one the model answers with.
            let Some((_, band)) = hits.iter().filter(|(_, b)| b.range.start.col <= lo && lo <= b.range.end.col).max_by_key(|(i, _)| *i) else {
                continue;
            };
            let last = hi.saturating_sub(1);
            match runs.last_mut() {
                Some(run) if run.style == band.style && run.last.saturating_add(1) == lo => run.last = last,
                _ => runs.push(BandRun { next: lo, last, style: band.style }),
            }
        }
        runs
    }

    /// The next band column of the row being written, if any is left.
    fn peek_band(&mut self) -> Option<(u32, StyleId)> {
        loop {
            let run = self.band_runs.get_mut(self.band_at)?;
            if run.next > run.last {
                self.band_at += 1;
                continue;
            }
            return Some((run.next, run.style));
        }
    }

    fn take_band(&mut self) {
        if let Some(run) = self.band_runs.get_mut(self.band_at) {
            run.next = run.next.saturating_add(1);
        }
    }

    /// Writes the styled blank of a band column: `<c r="B3" s="7"/>`, and nothing at the default
    /// style (a format the cell would have anyway is not a format).
    fn write_band_cell(&mut self, col: u32, style: StyleId) {
        let s = self.styles.get(style);
        if s == 0 {
            return;
        }
        self.xml.push_str("<c r=\"");
        a1_into(&mut self.xml, CellRef::new(self.band_row, col));
        self.xml.push_str("\" s=\"");
        push_u32(&mut self.xml, s);
        self.xml.push_str("\"/>");
    }

    /// Writes every band column before `before` (all of them when `None`), in column order.
    fn flush_bands(&mut self, before: Option<u32>) {
        while let Some((col, style)) = self.peek_band() {
            if let Some(limit) = before
                && col >= limit
            {
                break;
            }
            self.take_band();
            self.write_band_cell(col, style);
        }
    }
}

impl RowVisitor for RowBuilder<'_> {
    fn row_open(&mut self, row: u32, info: &LineInfo) {
        self.band_row = row;
        self.band_runs = self.band_runs_for(row);
        self.band_at = 0;
        write_row_open(&mut self.xml, row, info, self.styles);
    }
    fn row_empty(&mut self) {
        // A row the walker thought empty can still be a band's: the band cells are written here.
        if self.peek_band().is_some() {
            self.xml.push('>');
            self.flush_bands(None);
            self.xml.push_str("</row>");
        } else {
            self.xml.push_str("/>");
        }
    }
    fn row_cells(&mut self) {
        self.xml.push('>');
    }
    fn row_close(&mut self) {
        self.flush_bands(None);
        self.xml.push_str("</row>");
    }
    fn cell(&mut self, at: CellRef, cell: Option<&Cell>, a: &Additions<'_>) -> Result<(), IoError> {
        // The file wants a row's cells in column order: the band's blanks before this cell first,
        // and a band column the same as the cell is the cell's (its own style wins).
        self.flush_bands(Some(at.col));
        while let Some((col, _)) = self.peek_band() {
            if col == at.col {
                self.take_band();
            } else {
                break;
            }
        }
        self.push_cell(at, cell, a)
    }
}

/// Rows a window holds at least: below this the fixed cost of a window (its own walk, its own
/// buffers) costs more than another thread saves.
#[cfg(not(target_arch = "wasm32"))]
const MIN_WINDOW_ROWS: u32 = 4096;
/// Cells a sheet needs before its rows are built on more than one thread: below this one walk is
/// shorter than starting the threads.
#[cfg(not(target_arch = "wasm32"))]
const PARALLEL_MIN_CELLS: usize = 20_000;
/// Threads a sheet's rows are split over at most: past this a save contends for memory bandwidth
/// more than it gains.
#[cfg(not(target_arch = "wasm32"))]
const MAX_WORKERS: usize = 8;

/// How many threads to build a save's rows on: the machine's parallelism, capped. wasm has no OS
/// threads, so its rows are always built on the calling thread.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn default_workers() -> usize {
    std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1).clamp(1, MAX_WORKERS)
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn default_workers() -> usize {
    1
}

/// The last row a sheet writes: its last cell in the store, an addition (a spilled value, a table
/// header, a picture), a row it has line info for, or one a filter hides.
fn last_written_row(sheet: &Sheet, additions: &BTreeMap<u32, Vec<(u32, Additions<'_>)>>) -> u32 {
    // The sheet's own used range, not the store's: a style band writes styled blanks on rows the
    // store holds nothing for (gaps §32, slice 3b).
    let store = sheet.used_range().map(|u| u.end.row);
    let lines = sheet.rows.keys().last();
    let hidden = sheet.filter_hidden.rows().last();
    let additions = additions.keys().next_back().copied();
    [store, lines, hidden, additions].into_iter().flatten().max().unwrap_or(0)
}

/// The row windows a sheet is built in, in order: one per worker, never smaller than
/// [`MIN_WINDOW_ROWS`] rows, and one window for a sheet too small to pay for the threads.
#[cfg(not(target_arch = "wasm32"))]
fn row_windows(last_row: u32, cells: usize, workers: usize) -> Vec<(u32, u32)> {
    let workers = workers.clamp(1, MAX_WORKERS);
    if workers < 2 || cells < PARALLEL_MIN_CELLS {
        return vec![(0, last_row)];
    }
    let rows = u64::from(last_row) + 1;
    let size = (rows / workers as u64).max(u64::from(MIN_WINDOW_ROWS)).min(rows) as u32;
    let mut windows = Vec::with_capacity(workers);
    let mut first = 0u32;
    loop {
        let last = first.saturating_add(size.saturating_sub(1)).min(last_row);
        windows.push((first, last));
        if last >= last_row {
            break;
        }
        first = last.saturating_add(1);
    }
    if windows.len() < 2 {
        return vec![(0, last_row)];
    }
    windows
}

/// The window a sheet with one window writes: the whole of it.
fn sole_window(sheet: &Sheet, additions: &BTreeMap<u32, Vec<(u32, Additions<'_>)>>) -> (u32, u32) {
    (0, last_written_row(sheet, additions))
}

/// The windows a sheet is built in (see [`row_windows`]). wasm has no OS threads: one window,
/// built on the calling thread.
#[cfg(not(target_arch = "wasm32"))]
fn window_plan(sheet: &Sheet, additions: &BTreeMap<u32, Vec<(u32, Additions<'_>)>>, workers: usize) -> Vec<(u32, u32)> {
    row_windows(last_written_row(sheet, additions), sheet.cells.len(), workers)
}

#[cfg(target_arch = "wasm32")]
fn window_plan(sheet: &Sheet, additions: &BTreeMap<u32, Vec<(u32, Additions<'_>)>>, _workers: usize) -> Vec<(u32, u32)> {
    vec![sole_window(sheet, additions)]
}

/// Runs `each` over the windows on worker threads and returns their results in window order. A
/// worker that panicked is a failed save, not a part with a hole in it: the join reports it and the
/// panic payload is dropped inside the scope, so nothing unwinds out of it.
#[cfg(not(target_arch = "wasm32"))]
fn in_windows<T, F>(windows: &[(u32, u32)], what: &str, each: F) -> Vec<Result<T, IoError>>
where
    T: Send,
    F: Fn((u32, u32)) -> Result<T, IoError> + Send + Sync,
{
    std::thread::scope(|scope| {
        let each = &each;
        let handles: Vec<_> = windows.iter().map(|&rows| scope.spawn(move || each(rows))).collect();
        handles.into_iter().map(|h| h.join().unwrap_or_else(|_| Err(IoError::Zip(format!("a worksheet {what} panicked"))))).collect()
    })
}

/// The texts a sheet writes, window by window and merged in row order: the shared string table's
/// order is the file's first-use order, so the windows have to be put back in that order before
/// their strings are interned.
#[cfg(not(target_arch = "wasm32"))]
fn collect_strings(sheet: &Sheet, additions: &BTreeMap<u32, Vec<(u32, Additions<'_>)>>, windows: &[(u32, u32)]) -> Result<SstBatch, IoError> {
    if windows.len() < 2 {
        return collect_window(sheet, additions, windows.first().copied().unwrap_or_else(|| sole_window(sheet, additions)));
    }
    let mut merged = SstBatch::default();
    for batch in in_windows(windows, "shared string collector", |rows| collect_window(sheet, additions, rows)) {
        merged.append(batch?);
    }
    Ok(merged)
}

#[cfg(target_arch = "wasm32")]
fn collect_strings(sheet: &Sheet, additions: &BTreeMap<u32, Vec<(u32, Additions<'_>)>>, windows: &[(u32, u32)]) -> Result<SstBatch, IoError> {
    collect_window(sheet, additions, windows.first().copied().unwrap_or_else(|| sole_window(sheet, additions)))
}

/// One window of a sheet's rows, built on this thread.
fn build_window(
    wb: &Workbook,
    si: usize,
    sheet: &Sheet,
    additions: &BTreeMap<u32, Vec<(u32, Additions<'_>)>>,
    sst: &Sst,
    styles: StyleIds,
    rows: (u32, u32),
) -> Result<(String, RowFlags), IoError> {
    let mut builder = RowBuilder::new(wb, si, styles, sst);
    walk_rows(sheet, RangeRef::rows(rows.0, rows.1), additions, &mut builder)?;
    Ok((builder.xml, builder.flags))
}

/// A sheet's `<sheetData>` content, one `String` per window in row order, and the flags the
/// workbook assembly reads back. A sheet big enough is built in row windows, one per thread; the
/// caller either streams the windows to the compression thread as it joins them or concatenates
/// them, and either way the bytes are the one walk a single thread would have produced.
#[cfg(not(target_arch = "wasm32"))]
fn build_rows(
    wb: &Workbook,
    si: usize,
    sheet: &Sheet,
    additions: &BTreeMap<u32, Vec<(u32, Additions<'_>)>>,
    sst: &Sst,
    styles: StyleIds,
    windows: &[(u32, u32)],
) -> Result<(Vec<String>, RowFlags), IoError> {
    if windows.len() < 2 {
        let (xml, flags) =
            build_window(wb, si, sheet, additions, sst, styles, windows.first().copied().unwrap_or_else(|| sole_window(sheet, additions)))?;
        return Ok((vec![xml], flags));
    }
    let built = in_windows(windows, "row builder", |rows| build_window(wb, si, sheet, additions, sst, styles, rows));
    let mut xml = Vec::with_capacity(built.len());
    let mut flags = RowFlags::default();
    for window in built {
        let (part, window_flags) = window?;
        flags.needs_calc |= window_flags.needs_calc;
        flags.dynamic_arrays |= window_flags.dynamic_arrays;
        xml.push(part);
    }
    Ok((xml, flags))
}

/// [`build_rows`] for wasm: no OS threads, so the rows are built on the calling thread.
#[cfg(target_arch = "wasm32")]
fn build_rows(
    wb: &Workbook,
    si: usize,
    sheet: &Sheet,
    additions: &BTreeMap<u32, Vec<(u32, Additions<'_>)>>,
    sst: &Sst,
    styles: StyleIds,
    windows: &[(u32, u32)],
) -> Result<(Vec<String>, RowFlags), IoError> {
    let (xml, flags) =
        build_window(wb, si, sheet, additions, sst, styles, windows.first().copied().unwrap_or_else(|| sole_window(sheet, additions)))?;
    Ok((vec![xml], flags))
}

/// The formula a cell writes: an override (a table header's text, a spilled value) replaces the
/// cell, formula and all.
fn formula_of<'c>(cell: Option<&'c Cell>, a: &Additions<'_>) -> Option<&'c cella_model::Formula> {
    if a.override_text.is_some() {
        return None;
    }
    cell.and_then(|x| x.formula.as_deref())
}

/// What a cell writes, before the format's own mapping: the overrides the row walk folds in (a table
/// header's text, a spilled value), an array's top-left, and the two cached errors the file format
/// cannot hold (`#CIRC!` is written as 0, and `#SPILL!`/`#CALC!` as `#VALUE!`, which is what Excel
/// itself caches). The shared-string pre-pass and the row builder both resolve a cell here, so the
/// table cannot drift from the XML.
fn cell_value(cell: Option<&Cell>, ov: Option<&String>, extra: Option<&Value>) -> Value {
    let value: Value = match (ov, extra) {
        (Some(t), _) => Value::text(t.as_str()),
        (None, Some(v)) => v.clone(),
        _ => cell.map(|x| x.value.scalar()).unwrap_or(Value::Empty),
    };
    let value = match value {
        Value::Array(a) => a.get(0, 0).cloned().unwrap_or(Value::Empty),
        v => v,
    };
    match value {
        Value::Error(CellError::Circ) => Value::Number(0.0),
        Value::Error(CellError::Spill | CellError::Calc) => Value::Error(CellError::Value),
        v => v,
    }
}

/// What a cell needs beyond the store: a table header's text, a spilled value, a picture, or the
/// range a dynamic array formula spills into. All are sparse, which is why they are looked up in a
/// per-row list during the one ordered walk rather than in a map keyed by cell.
#[derive(Clone, Default)]
struct Additions<'a> {
    override_text: Option<&'a String>,
    value: Option<&'a Value>,
    /// The `vm` id of the cell's static picture (a rich-data value), assigned in the sheet's cell
    /// order before any row is built.
    picture_vm: Option<u32>,
    dynamic: Option<RangeRef>,
}
/// The additions for one cell, adding the entry when it isn't there yet.
fn touch<'b, 'a>(additions: &'b mut BTreeMap<u32, Vec<(u32, Additions<'a>)>>, c: CellRef) -> &'b mut Additions<'a> {
    let row = additions.entry(c.row).or_default();
    let at = match row.binary_search_by_key(&c.col, |(col, _)| *col) {
        Ok(i) => i,
        Err(i) => {
            row.insert(i, (c.col, Additions::default()));
            i
        }
    };
    &mut row[at].1
}
/// `<row …` up to the point where its cells (or `/>`) follow.
fn write_row_open(s: &mut String, r: u32, info: &LineInfo, styles: StyleIds) {
    s.push_str("<row r=\"");
    push_u32(s, r + 1);
    s.push('"');
    if let Some(st) = info.style {
        s.push_str(" s=\"");
        push_u32(s, styles.get(st));
        s.push_str("\" customFormat=\"1\"");
    }
    if let Some(h) = info.size {
        let _ = write!(s, " ht=\"{}\"{}", num(px_to_pt(h)), if info.custom { " customHeight=\"1\"" } else { "" });
    }
    if info.hidden {
        s.push_str(" hidden=\"1\"");
    }
    if info.outline > 0 {
        let _ = write!(s, " outlineLevel=\"{}\"", info.outline.min(7));
    }
    if info.collapsed {
        s.push_str(" collapsed=\"1\"");
    }
}
/// `c.a1()` appended without the `String` it would allocate: this runs once per cell.
fn a1_into(s: &mut String, c: CellRef) {
    let mut col = c.col as usize + 1;
    let mut letters = [0u8; 3];
    let mut n = 0;
    while col > 0 {
        letters[n] = b'A' + ((col - 1) % 26) as u8;
        n += 1;
        col = (col - 1) / 26;
    }
    for i in (0..n).rev() {
        s.push(letters[i] as char);
    }
    push_u32(s, c.row + 1);
}
/// `n` in decimal, appended without the formatting machinery.
fn push_u32(s: &mut String, n: u32) {
    push_u64(s, u64::from(n));
}
fn push_u64(s: &mut String, mut n: u64) {
    let mut digits = [0u8; 20];
    let mut i = digits.len();
    loop {
        i -= 1;
        digits[i] = b'0' + (n % 10) as u8;
        n /= 10;
        if n == 0 {
            break;
        }
    }
    s.push_str(std::str::from_utf8(&digits[i..]).unwrap_or("0"));
}
/// [`num`] appended to the buffer: a report writes tens of millions of these, and the `String` that
/// `num` builds (and, for a fraction, the `replace` that builds a second one) is most of the cost.
fn push_num(s: &mut String, n: f64) {
    if !n.is_finite() {
        s.push('0');
        return;
    }
    if n == n.trunc() && n.abs() < 1e15 {
        let v = n as i64;
        if v < 0 {
            s.push('-');
        }
        push_u64(s, v.unsigned_abs());
        return;
    }
    use std::fmt::Write as _;
    let start = s.len();
    let _ = write!(s, "{n:?}");
    if s[start..].contains('e') {
        let replaced = s[start..].replace('e', "E");
        s.truncate(start);
        s.push_str(&replaced);
    }
}
fn write_cell(
    s: &mut String,
    styles: StyleIds,
    sst: &Sst,
    c: CellRef,
    cell: Option<&Cell>,
    a: &Additions<'_>,
    dynamic: Option<&RangeRef>,
    flags: &mut RowFlags,
) -> Result<(), IoError> {
    let style = cell.map(|x| styles.get(x.style)).unwrap_or(0);
    s.push_str("<c r=\"");
    a1_into(s, c);
    s.push('"');
    if style != 0 {
        s.push_str(" s=\"");
        push_u32(s, style);
        s.push('"');
    }
    if let Some(vm) = a.picture_vm {
        s.push_str(" t=\"e\" vm=\"");
        push_u32(s, vm);
        s.push_str("\"><v>#VALUE!</v></c>");
        return Ok(());
    }
    let formula = formula_of(cell, a);
    let value = cell_value(cell, a.override_text, a.value);
    // A formula read with dynamic-array metadata keeps it, even where the shape analysis would
    // not need it: Excel decided the formula is evaluated as an array.
    let own = formula.filter(|f| f.dynamic && f.array.is_none()).map(|_| RangeRef::cell(c));
    let dynamic = dynamic.or(own.as_ref());
    // A non-spilling formula may still need array evaluation to avoid implicit intersection.
    if dynamic.is_some() && formula.is_some_and(|f| f.array.is_none()) {
        s.push_str(" cm=\"1\"");
        flags.dynamic_arrays = true;
    }
    if let Some(f) = formula {
        let t = match &value {
            Value::Text(_) => " t=\"str\"",
            Value::Bool(_) => " t=\"b\"",
            Value::Error(_) => " t=\"e\"",
            _ => "",
        };
        s.push_str(t);
        s.push('>');
        match (f.array.as_deref(), dynamic) {
            // An array formula names the range its result occupies.
            (Some(r), _) => {
                s.push_str("<f t=\"array\" ref=\"");
                s.push_str(&r.a1());
                s.push_str("\">");
            }
            (None, Some(r)) => {
                s.push_str("<f t=\"array\" ref=\"");
                s.push_str(&r.a1());
                s.push_str("\">");
            }
            _ => s.push_str("<f>"),
        }
        crate::xml::push_esc(s, &crate::fmla::file_text(f, c));
        s.push_str("</f>");
        match &value {
            Value::Number(n) => {
                s.push_str("<v>");
                push_num(s, *n);
                s.push_str("</v>");
            }
            Value::Text(t) => {
                s.push_str("<v>");
                crate::xml::push_esc(s, &encode_xstring(t));
                s.push_str("</v>");
            }
            Value::Bool(b) => {
                s.push_str(if *b { "<v>1</v>" } else { "<v>0</v>" });
            }
            Value::Error(e) => {
                s.push_str("<v>");
                s.push_str(e.as_str());
                s.push_str("</v>");
            }
            _ => flags.needs_calc = true,
        }
        s.push_str("</c>");
        return Ok(());
    }
    match &value {
        Value::Empty | Value::Array(_) => s.push_str("/>"),
        Value::Number(n) => {
            s.push_str("><v>");
            push_num(s, *n);
            s.push_str("</v></c>");
        }
        Value::Text(t) => {
            // The pre-pass counted this string and gave it its id; a miss here would be a writer
            // bug, and writing a reference the table does not hold would make the file invalid.
            let i = sst.id(t).ok_or_else(|| IoError::Format("a worksheet text cell is missing from the shared string table".into()))?;
            s.push_str(" t=\"s\"><v>");
            push_u32(s, i);
            s.push_str("</v></c>");
        }
        Value::Bool(b) => {
            s.push_str(if *b { " t=\"b\"><v>1</v></c>" } else { " t=\"b\"><v>0</v></c>" });
        }
        Value::Error(e) => {
            s.push_str(" t=\"e\"><v>");
            s.push_str(e.as_str());
            s.push_str("</v></c>");
        }
    }
    Ok(())
}
const X14_NS: &str = "http://schemas.microsoft.com/office/spreadsheetml/2009/9/main";
const XM_NS: &str = "http://schemas.microsoft.com/office/excel/2006/main";
/// The box a note made here gets: two columns right of its cell, four rows tall, 108 x 59.25 pt.
/// A box read from a file that equals it is kept as `None`, so a note's model survives a save.
pub(crate) const DEFAULT_NOTE_BOX: NoteBox = NoteBox {
    from: NoteCorner { col: 1, col_px: 15, row: 0, row_px: 2 },
    to: NoteCorner { col: 3, col_px: 15, row: 4, row_px: 16 },
    size_pt: Some(DEFAULT_NOTE_SIZE_PT),
};
const DEFAULT_NOTE_SIZE_PT: (f64, f64) = (108.0, 59.25);
// The window plan is native-only: wasm builds a sheet's rows in one window on the calling thread.
#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    /// The window plan: one window per worker for a sheet long enough and big enough to pay for the
    /// threads, one window for a short sheet or a small one, and never a window below the minimum
    /// size.
    #[test]
    fn row_windows_split_long_sheets_and_keep_short_ones_whole() {
        let eight: Vec<(u32, u32)> = (0..8).map(|i| (i * 125_000, (i + 1) * 125_000 - 1)).collect();
        assert_eq!(row_windows(999_999, 8_000_000, 8), eight);
        // Shorter than eight minimum-size windows: three, the last one cut at the sheet's end.
        assert_eq!(row_windows(11_999, 24_000, 8), vec![(0, 4095), (4096, 8191), (8192, 11_999)]);
        // A short sheet, and a sheet with too few cells to pay for the threads: one window.
        assert_eq!(row_windows(100, 24_000, 8), vec![(0, 100)]);
        assert_eq!(row_windows(11_999, 1_000, 8), vec![(0, 11_999)]);
        assert_eq!(row_windows(0, 8_000_000, 8), vec![(0, 0)]);
        // One worker asked for is one window, however long the sheet.
        assert_eq!(row_windows(999_999, 8_000_000, 1), vec![(0, 999_999)]);
        // More workers than the cap are clamped.
        assert_eq!(row_windows(999_999, 8_000_000, 64).len(), MAX_WORKERS);
    }

    /// A worker that panics fails the save with an error: the panic is caught by the join and
    /// reported, so it neither unwinds through the save nor leaves a window missing from the part.
    #[test]
    fn a_panicking_window_is_an_error() {
        let windows = [(0u32, 9u32), (10, 19)];
        let results = in_windows(&windows, "row builder", |rows| {
            assert_eq!(rows.0 % 10, 0);
            if rows.0 == 10 {
                panic!("the second window panics");
            }
            Ok(rows.0)
        });
        assert!(matches!(&results[0], Ok(0)), "the window that finished keeps its result");
        assert!(matches!(&results[1], Err(IoError::Zip(_))), "a panicked worker is an error");
    }

    /// The packed string pre-pass answers exactly what the cell-by-cell walk answers, on a sheet
    /// with every kind of cell and addition a save builds: values, texts (unique and repeated),
    /// formulas with and without a cached result, stored value arrays (whose first element is the
    /// value the cell writes), a legacy array formula, styled blanks, and the additions the
    /// writer merges in — a table header's override text over a stored text and over a stored
    /// formula, a spilled value over a blank and over a hole and below the data, a picture, a
    /// dynamic anchor. The windows cut between the rows and inside them.
    ///
    /// The fixture also pins the flag the first version of the packed collector keyed on:
    /// `array_anchors` is raised by a legacy array **formula** and not by a stored value array,
    /// so the collector may not branch on it.
    #[test]
    fn packed_string_prepass_matches_the_cell_walk() {
        let mut seed = 0x2545_F491_4F6C_DD1Du64;
        let mut next = move || {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            (seed >> 33) as u32
        };
        let mut sheet = Sheet::new("Data");
        for row in 0..40u32 {
            for col in 0..8u32 {
                let at = CellRef::new(row, col);
                let kind = next() % 11;
                let cell = match kind {
                    0 => Cell::value(Value::Number(f64::from(next() % 1000))),
                    1 => Cell::value(Value::text(format!("text {row}/{col}"))),
                    2 => Cell::value(Value::Bool(next() % 2 == 0)),
                    3 => Cell::value(Value::Error(CellError::Div0)),
                    4 => Cell::formula(cella_model::Formula::new("=A1*2")),
                    5 => Cell { value: Value::text("cached text result"), ..Cell::formula(cella_model::Formula::new("=A1&\" says\"")) },
                    6 => Cell { style: cella_model::StyleId(1), ..Default::default() },
                    7 => Cell::value(Value::Array(std::sync::Arc::new(cella_core::Array::row(vec![Value::text("array text")])))),
                    8 => Cell::value(Value::Array(std::sync::Arc::new(cella_core::Array::row(vec![Value::Number(9.0)])))),
                    9 => Cell::value(Value::text("repeated text")),
                    _ => continue,
                };
                sheet.set_cell(at, cell);
            }
        }
        // A legacy array formula (Ctrl+Shift+Enter): a multi-cell `f.array`, whose anchor the
        // store keeps. Both collectors skip it by its formula, and it is what raises the flag.
        let mut legacy = cella_model::Formula::new("=A1*2");
        legacy.array = Some(Box::new(RangeRef::parse("C5:E5").expect("a range")));
        sheet.set_cell(CellRef::new(4, 2), Cell::formula(legacy));
        assert!(sheet.cells.has_array_anchors(), "the legacy array formula raises the anchor flag");
        // ... and a stored value array does not: the flag names legacy formulas, not values, so a
        // collector may not use it to decide whether a packed `None` needs resolving.
        let mut values_only = Sheet::new("Values");
        values_only
            .set_cell(CellRef::new(0, 0), Cell::value(Value::Array(std::sync::Arc::new(cella_core::Array::row(vec![Value::text("array text")])))));
        assert!(!values_only.cells.has_array_anchors(), "a stored value array is no legacy array formula");

        // Additions the way `write_sheet` builds them, in the kinds it builds.
        let header = String::from("Header");
        let spilled_text = Value::text("spilled text");
        let spilled_number = Value::Number(7.0);
        let dynamic = RangeRef::parse("B2:D4").expect("a range");
        let mut additions: BTreeMap<u32, Vec<(u32, Additions<'_>)>> = BTreeMap::new();
        touch(&mut additions, CellRef::new(0, 1)).override_text = Some(&header);
        touch(&mut additions, CellRef::new(5, 0)).override_text = Some(&header);
        touch(&mut additions, CellRef::new(10, 3)).value = Some(&spilled_text);
        touch(&mut additions, CellRef::new(20, 5)).value = Some(&spilled_number);
        touch(&mut additions, CellRef::new(45, 0)).value = Some(&spilled_text);
        touch(&mut additions, CellRef::new(15, 2)).picture_vm = Some(1);
        touch(&mut additions, CellRef::new(25, 4)).dynamic = Some(dynamic);

        for (a, b) in [(0u32, 60u32), (0, 0), (4, 6), (9, 12), (14, 15), (29, 60)] {
            let packed = collect_window(&sheet, &additions, (a, b)).expect("the packed window collects");
            let walked = collect_window_cells(&sheet, &additions, (a, b)).expect("the cell window collects");
            assert_eq!(packed.refs, walked.refs, "refs, window {a}..={b}");
            let same = packed.strings.iter().map(|s| &**s).eq(walked.strings.iter().map(|s| &**s));
            assert!(same, "strings, window {a}..={b}");
        }
    }
}

#[cfg(test)]
mod perf {
    use super::*;

    /// A workbook shaped like the perf example's own: a text column, four number columns, and two
    /// formula columns, on one sheet.
    fn big_workbook(rows: u32) -> Workbook {
        let mut sheet = Sheet::new("Big");
        for r in 0..rows {
            let n = f64::from(r);
            let row = r + 1;
            sheet.set_value(CellRef::new(r, 0), Value::text(format!("Item {r}")));
            sheet.set_value(CellRef::new(r, 1), Value::Number(n));
            sheet.set_value(CellRef::new(r, 2), Value::Number(n % 97.0));
            sheet.set_value(CellRef::new(r, 3), Value::Number((n % 13.0) * 1.5));
            sheet.set_value(CellRef::new(r, 4), Value::Number(1.0));
            sheet.set_value(CellRef::new(r, 5), Value::Number(2.0));
            let g = format!("B{row}*C{row}+D{row}");
            let h = if r == 0 { format!("G{row}") } else { format!("H{}+G{row}", row - 1) };
            for (col, text) in [(6u32, g), (7, h)] {
                let formula = cella_model::Formula::new(&text);
                let value = Value::Number(n * 2.0);
                sheet.set_cell(
                    CellRef::new(r, col),
                    Cell { value, formula: Some(std::sync::Arc::new(formula)), style: cella_model::StyleId::DEFAULT },
                );
            }
        }
        let mut wb = Workbook::new();
        wb.sheets.clear();
        wb.sheets.push(std::sync::Arc::new(sheet));
        wb
    }

    /// Where a save's time goes: the worksheet XML build (one `write_sheet` per sheet) against the
    /// whole write, which also deflates every sheet. A synthetic workbook of CELLA_PERF_ROWS
    /// rows (default 1,000,000) unless CELLA_PERF_FILE points at a real file (the clock is read
    /// here, which the crate bans on wasm):
    ///   cargo test --release -p cella-xlsx --lib -- --ignored --nocapture saving_a_big_file
    ///   CELLA_PERF_FILE=big.xlsx ... saving_a_big_file
    #[test]
    #[ignore = "timing, not a check"]
    #[cfg_attr(not(target_arch = "wasm32"), allow(clippy::disallowed_methods))]
    fn saving_a_big_file() {
        let wb = match std::env::var("CELLA_PERF_FILE") {
            Ok(path) => crate::read_xlsx_file(std::path::Path::new(&path)).expect("read").0,
            Err(_) => {
                let rows: u32 = std::env::var("CELLA_PERF_ROWS").ok().and_then(|v| v.parse().ok()).unwrap_or(1_000_000);
                big_workbook(rows)
            }
        };
        let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1);
        println!("  {} cores, {} sheets", threads, wb.sheets.len());
        let mut out = crate::write::Out::default();
        let mut build = std::time::Duration::ZERO;
        let mut xml_bytes = 0usize;
        for (i, sheet) in wb.sheets.iter().enumerate() {
            let t = std::time::Instant::now();
            let name = format!("xl/worksheets/sheet{}.xml", i + 1);
            let (xml, _) =
                write_sheet(&wb, i, i == wb.active_sheet, &mut out, &crate::slicer::SlicerWriter::default(), default_workers(), &name, false)
                    .expect("sheet");
            build += t.elapsed();
            xml_bytes += xml.len();
            println!(
                "  {:<28} {:>10.1} MB xml   {:>8.2?}   {:.0} ns a cell",
                sheet.name.chars().take(28).collect::<String>(),
                xml.len() as f64 / 1048576.0,
                t.elapsed(),
                t.elapsed().as_secs_f64() * 1e9 / sheet.cells.len().max(1) as f64
            );
        }
        let t = std::time::Instant::now();
        let bytes = crate::write_xlsx(&wb).expect("write");
        let total = t.elapsed();
        println!(
            "  XML build {:.2?} ({:.0} MB)   total write_xlsx {:.2?} ({:.0} MB out)   build is {:.0} % of the save",
            build,
            xml_bytes as f64 / 1048576.0,
            total,
            bytes.len() as f64 / 1048576.0,
            100.0 * build.as_secs_f64() / total.as_secs_f64().max(f64::EPSILON)
        );

        // The biggest worksheet's XML through the zip writer at several levels: what compression
        // costs and what it buys, on this file's real text rather than a synthetic row. Off unless
        // asked, because it takes minutes on a million-row sheet.
        if std::env::var_os("CELLA_PERF_ZIP").is_none() {
            return;
        }
        if let Some(sheet) = wb.sheets.iter().max_by_key(|s| s.cells.len()) {
            let mut out = crate::write::Out::default();
            let (xml, _) = write_sheet(
                &wb,
                wb.sheets.iter().position(|s| std::ptr::eq(&**s, &**sheet)).unwrap_or(0),
                false,
                &mut out,
                &crate::slicer::SlicerWriter::default(),
                default_workers(),
                "xl/worksheets/sheet.xml",
                false,
            )
            .expect("sheet");
            let data = xml.as_bytes();
            println!("  {} ({:.0} MB), through the zip writer:", sheet.name, data.len() as f64 / 1048576.0);
            for level in [1, 3, 6, 9] {
                let t = std::time::Instant::now();
                let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
                let options = zip::write::SimpleFileOptions::default()
                    .compression_method(zip::CompressionMethod::Deflated)
                    .compression_level(Some(level))
                    .large_file(true);
                use std::io::Write as _;
                zip.start_file("xl/worksheets/sheet.xml", options).expect("start");
                zip.write_all(data).expect("write");
                let packed = zip.finish().expect("finish").into_inner();
                let secs = t.elapsed().as_secs_f64();
                println!(
                    "    level {level}   {:>8.2?}   {:>5.0} MB/s   {:.0} MB packed",
                    t.elapsed(),
                    data.len() as f64 / 1048576.0 / secs,
                    packed.len() as f64 / 1048576.0
                );
            }
        }
    }

    /// Where a saved report's time goes. Run it with:
    ///   cargo test --release -p cella-xlsx --lib -- --ignored --nocapture number_formatting
    #[test]
    #[ignore = "timing, not a check"]
    #[cfg_attr(not(target_arch = "wasm32"), allow(clippy::disallowed_methods))]
    fn number_formatting() {
        // The mix a report has: whole counts and two-decimal money.
        let sample: Vec<f64> = (0..1_000_000).map(|i| if i % 3 == 0 { i as f64 } else { i as f64 * 0.0188 + 0.5 }).collect();
        let mut s = String::with_capacity(32);
        let t = std::time::Instant::now();
        for v in &sample {
            s.clear();
            let _ = write!(s, "{v:?}");
        }
        println!("write!({{:?}})  {:?}  ({:.0} ns a number)", t.elapsed(), t.elapsed().as_secs_f64() * 1e9 / sample.len() as f64);
        let t = std::time::Instant::now();
        for v in &sample {
            s.clear();
            push_num(&mut s, *v);
        }
        println!("push_num      {:?}  ({:.0} ns a number)", t.elapsed(), t.elapsed().as_secs_f64() * 1e9 / sample.len() as f64);
        // And the same numbers through `num`, which builds a String first.
        let t = std::time::Instant::now();
        let mut len = 0usize;
        for v in &sample {
            len += num(*v).len();
        }
        println!("num() + String {:?}  ({:.0} ns a number, {len} bytes)", t.elapsed(), t.elapsed().as_secs_f64() * 1e9 / sample.len() as f64);
    }
}
