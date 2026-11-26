//! Reading a PivotTable and its cache back into the model's [`PivotTable`].

use super::*;

// ================================================================ reading

/// A cache field's `fieldGroup`: the base database field, its grouping and its item captions.
#[derive(Clone, Debug)]
struct FieldGroupIn {
    base: Option<usize>,
    /// A date part; `None` with `number` set, or with neither (an unsupported grouping).
    date: PivotDateGroup,
    number: Option<PivotNumberGroup>,
    items: Vec<String>,
}
#[derive(Clone, Debug)]
struct FieldIn {
    name: String,
    database: bool,
    /// Shared item captions.
    items: Vec<String>,
    group: Option<FieldGroupIn>,
    /// A calculated field's formula.
    formula: Option<String>,
    /// The calculated items among the shared items: their index and caption (`f="1"`).
    calc: Vec<(u32, String)>,
}
impl FieldIn {
    fn labels(&self) -> &[String] {
        match &self.group {
            Some(g) if !g.items.is_empty() => &g.items,
            _ => &self.items,
        }
    }
}
/// A calculated item as the cache stores it: its field, its index among that field's shared
/// items, and the formula in Excel's `Field[item]` form.
#[derive(Clone, Debug)]
struct CalcIn {
    field: usize,
    item: usize,
    formula: String,
}
#[derive(Clone, Debug, Default)]
struct CacheIn {
    source: String,
    /// Why Cella can't compute this report (its cache is not a worksheet range): the file's
    /// cached values are kept, the source is never queried. `None` for a range cache.
    read_only_reason: Option<String>,
    fields: Vec<FieldIn>,
    calc_items: Vec<CalcIn>,
}
/// Workbook-level pivot cache lookup, filled from `<pivotCaches>`; caches are parsed once.
#[derive(Default)]
pub struct CachesIn {
    by_id: HashMap<u32, String>,
    parsed: HashMap<String, Option<CacheIn>>,
    next_id: u32,
}
impl CachesIn {
    pub fn new(wb_xml: &El, wb_rels: &[crate::package::Rel]) -> CachesIn {
        let mut by_id = HashMap::new();
        if let Some(pc) = wb_xml.child("pivotCaches") {
            for c in pc.kids("pivotCache") {
                if let (Some(id), Some(rid)) = (c.attr_u32("cacheId"), c.attr("id"))
                    && let Some(r) = wb_rels.iter().find(|r| r.id == rid)
                {
                    by_id.insert(id, r.target.clone());
                }
            }
        }
        CachesIn { by_id, parsed: HashMap::new(), next_id: 1 }
    }
}
fn item_label_el(e: &El, sys: DateSystem) -> String {
    let v = e.attr("v").unwrap_or("");
    match e.name.as_str() {
        "m" => "(blank)".into(),
        "n" => v.trim().parse::<f64>().ok().filter(|n| n.is_finite()).map(number_to_text).unwrap_or_else(|| v.to_string()),
        "b" => if matches!(v.trim(), "1" | "true") { "TRUE" } else { "FALSE" }.into(),
        "d" => crate::sheet_read::parse_iso_datetime(v, sys).map(number_to_text).unwrap_or_else(|| v.to_string()),
        _ => v.to_string(),
    }
}
/// Whether a cache's `cacheSource` names the workbook's own data model: `xl/connections.xml` marks
/// the connection `type="5"` (`xlConnectionTypeMODEL`, what Excel calls "ThisWorkbookDataModel").
/// That is how a file Excel saved puts it — the cache only holds the connection id, and the model
/// itself hangs off the workbook.
fn is_model_connection(cx: &mut Ctx<'_>, src: &El) -> bool {
    let Some(id) = src.attr("connectionId") else { return false };
    let Ok(Some(bytes)) = cx.pkg.read_limited("xl/connections.xml", MAX_CONNECTIONS) else { return false };
    let Ok(conns) = crate::xml::parse(&bytes) else { return false };
    conns.kids("connection").any(|c| c.attr("id") == Some(id) && c.attr("type") == Some("5"))
}
/// Why a PivotTable whose cache is not a worksheet range is read-only, in words for the user.
/// Only a range cache (the one kind Cella computes) skips this.
fn not_computed_reason(cx: &mut Ctx<'_>, ty: &str, model: bool) -> String {
    match (ty, model) {
        ("olap", true) => "built on the workbook's data model (Power Pivot), which Cella keeps but does not query".to_string(),
        ("olap", false) => "built on an OLAP connection, which Cella keeps but does not query".to_string(),
        ("external", true) => "built on the workbook's data model (Power Pivot), which Cella keeps but does not query".to_string(),
        ("external", false) => "built on an external data source, which Cella keeps but does not query".to_string(),
        ("consolidation", _) => "built on several consolidated ranges, which Cella keeps but does not query".to_string(),
        _ => {
            cx.warn(format!("a PivotTable source of type \"{ty}\" is not computed by Cella; it is kept as it is"));
            format!("built on \"{ty}\" data, which Cella keeps but does not query")
        }
    }
}
fn read_cache(cx: &mut Ctx<'_>, part: &str, sheet_name: &str) -> Option<CacheIn> {
    let x = cx.optional_xml(part).ok().flatten()?;
    if x.name != "pivotCacheDefinition" {
        cx.warn(format!("{part} is not a pivot cache definition; its PivotTable was kept as plain cells"));
        return None;
    }
    let src = x.child("cacheSource");
    let ty = src.and_then(|s| s.attr("type")).unwrap_or("worksheet");
    // A cache Cella does not query (a data model, an OLAP cube, a connection): the report is
    // read as one Cella does not compute. Its cached values stay on the sheet, and the parts
    // the pass-through kept are written back as they came — never recomputed over a made-up range.
    // Two shapes name the workbook's own model: one related from the cache (`powerPivotData`), and
    // the one Excel writes, a connection in `xl/connections.xml` of type 5.
    let read_only_reason = (ty != "worksheet").then(|| {
        let linked = cx.pkg.rels(part).is_ok_and(|r| r.iter().any(|q| q.kind.eq_ignore_ascii_case("powerPivotData")));
        let model = linked || src.is_some_and(|s| is_model_connection(cx, s));
        not_computed_reason(cx, ty, model)
    });
    let ws = src.and_then(|s| s.child("worksheetSource"));
    let source = if read_only_reason.is_some() {
        String::new()
    } else {
        match ws {
            Some(w) if w.attr("name").is_some_and(|n| !n.trim().is_empty()) => w.attr("name").unwrap_or("").trim().to_string(),
            Some(w) => {
                if w.attr("id").is_some() {
                    cx.warn("a PivotTable's source is in another workbook; its source reference may not resolve");
                }
                let r = w.attr("ref").map(|r| r.replace('$', "")).and_then(|r| RangeRef::parse(&r));
                let Some(r) = r else {
                    cx.warn("a PivotTable's source reference is missing or invalid; kept as plain cells");
                    return None;
                };
                let sheet = w.attr("sheet").filter(|s| !s.is_empty()).unwrap_or(sheet_name);
                format!("{}!{}", quote_sheet(sheet), abs_range(&r))
            }
            None => {
                cx.warn("a PivotTable's cache has no worksheet source; kept as plain cells");
                return None;
            }
        }
    };
    let sys = cx.date_system;
    let mut fields = Vec::new();
    if let Some(cf) = x.child("cacheFields") {
        for f in cf.kids("cacheField").take(MAX_FIELDS) {
            let items = f.child("sharedItems").map(|si| si.children.iter().map(|e| item_label_el(e, sys)).collect()).unwrap_or_default();
            // A calculated item is marked `f="1"` on its shared item, after the real ones.
            let calc = f
                .child("sharedItems")
                .map(|si| {
                    si.children.iter().enumerate().filter(|(_, e)| e.flag("f", false)).map(|(i, e)| (i as u32, item_label_el(e, sys))).collect()
                })
                .unwrap_or_default();
            let group = f.child("fieldGroup").map(|g| {
                let base = g.attr_u32("base").map(|b| b as usize);
                let rp = g.child("rangePr");
                let by = rp.map(|r| r.attr("groupBy").unwrap_or("range"));
                let date = match by {
                    Some("years") => PivotDateGroup::Years,
                    Some("quarters") => PivotDateGroup::Quarters,
                    Some("months") => PivotDateGroup::Months,
                    Some("days") => PivotDateGroup::Days,
                    _ => PivotDateGroup::None,
                };
                // A number group is `groupBy="range"` with its three numbers ([MS-XLSX]
                // CT_RangePr `startNum`/`endNum`/`groupInterval`).
                let number = if date == PivotDateGroup::None && by == Some("range") {
                    let num = |k: &str| rp.and_then(|r| r.attr_f64(k));
                    match (num("startNum"), num("endNum"), num("groupInterval")) {
                        (Some(start), Some(end), Some(interval)) => Some(PivotNumberGroup { start, end, interval }),
                        _ => None,
                    }
                } else {
                    None
                };
                let gi = g.child("groupItems").map(|gi| gi.children.iter().map(|e| item_label_el(e, sys)).collect()).unwrap_or_default();
                FieldGroupIn { base, date, number, items: gi }
            });
            fields.push(FieldIn {
                name: f.attr("name").unwrap_or("").to_string(),
                database: f.flag("databaseField", true),
                items,
                group,
                formula: f.attr("formula").map(str::to_string),
                calc,
            });
        }
    }
    if fields.is_empty() {
        cx.warn("a PivotTable's cache has no fields; kept as plain cells");
        return None;
    }
    if let Some(reason) = &read_only_reason {
        cx.warn(format!("a PivotTable is read-only: it is {reason}. Its cached values are kept as they are"));
    }
    // Calculated items are a child of the cache definition: `Field[item]` references, the item's
    // index among its field's shared items, and the field's position on the axis.
    let mut calc_items: Vec<CalcIn> = Vec::new();
    if let Some(list) = x.child("calculatedItems") {
        for it in list.kids("calculatedItem").take(MAX_CALC_ITEMS) {
            let Some(formula) = it.attr("formula").map(str::to_string) else { continue };
            let Some(r) = it.child("pivotArea").and_then(|a| a.child("references")).and_then(|rs| rs.kids("reference").next()) else {
                cx.warn("a calculated item has no area reference; it was dropped");
                continue;
            };
            let (Some(field), Some(item)) = (r.attr_u32("field"), r.kids("x").next().and_then(|x| x.attr_u32("v"))) else {
                cx.warn("a calculated item has no item reference; it was dropped");
                continue;
            };
            calc_items.push(CalcIn { field: field as usize, item: item as usize, formula });
        }
    }
    Some(CacheIn { source, read_only_reason, fields, calc_items })
}
/// Reads the PivotTables related to a worksheet part.
pub fn read_pivots(cx: &mut Ctx<'_>, caches: &mut CachesIn, sheet_part: &str, sheet_name: &str) -> Result<Vec<PivotTable>, IoError> {
    let rels = cx.pkg.rels(sheet_part)?;
    let mut out = Vec::new();
    for rel in rels.iter().filter(|r| r.kind == "pivotTable" && !r.external) {
        let Some(x) = cx.optional_xml(&rel.target)? else { continue };
        if x.name != "pivotTableDefinition" {
            cx.warn(format!("{} is not a PivotTable definition; skipped", rel.target));
            continue;
        }
        // The cache: the table's own relationship, else the workbook's cache id.
        let trels = cx.pkg.rels(&rel.target)?;
        let cache_part = trels
            .iter()
            .find(|r| r.kind == "pivotCacheDefinition" && !r.external)
            .map(|r| r.target.clone())
            .or_else(|| x.attr_u32("cacheId").and_then(|id| caches.by_id.get(&id).cloned()));
        let Some(cache_part) = cache_part else {
            cx.warn(format!("PivotTable in {} has no cache; kept as plain cells", rel.target));
            continue;
        };
        let cache = match caches.parsed.get(&cache_part) {
            Some(c) => c.clone(),
            None => {
                let c = read_cache(cx, &cache_part, sheet_name);
                caches.parsed.insert(cache_part.clone(), c.clone());
                c
            }
        };
        let Some(cache) = cache else { continue };
        if let Some(pt) = read_table(cx, &x, &cache, caches.next_id) {
            caches.next_id = caches.next_id.saturating_add(1);
            out.push(pt);
        }
    }
    Ok(out)
}
/// A pivot field's data items (not subtotal or grand-total items).
fn items_of(e: &El) -> Vec<&El> {
    e.child("items").map(|it| it.kids("item").filter(|i| i.attr("t").is_none_or(|t| t == "data")).collect()).unwrap_or_default()
}
/// (source column name, date grouping) of a cache field used on an axis.
fn field_ref(cx: &mut Ctx<'_>, cache: &CacheIn, i: usize) -> Option<(String, PivotDateGroup, Option<PivotNumberGroup>)> {
    let f = cache.fields.get(i)?;
    if f.formula.is_some() {
        cx.warn("calculated PivotTable fields are not supported");
        return None;
    }
    match &f.group {
        Some(g) => {
            if g.date == PivotDateGroup::None && g.number.is_none() {
                cx.warn(format!("PivotTable grouping of \"{}\" is not supported; the field is ungrouped", f.name));
            }
            let base_name = if f.database { f.name.clone() } else { g.base.and_then(|b| cache.fields.get(b))?.name.clone() };
            Some((base_name, g.date, g.number))
        }
        None if !f.database => {
            cx.warn(format!("PivotTable field \"{}\" (grouped items) is not supported", f.name));
            None
        }
        None => Some((f.name.clone(), PivotDateGroup::None, None)),
    }
}
fn read_table(cx: &mut Ctx<'_>, x: &El, cache: &CacheIn, id: u32) -> Option<PivotTable> {
    let mut pt = PivotTable { id, ..PivotTable::default() };
    if let Some(reason) = &cache.read_only_reason {
        pt.source_kind = PivotSourceKind::DataModel;
        pt.read_only_reason = Some(reason.clone());
    }
    if let Some(n) = x.attr("name").filter(|n| !n.trim().is_empty()) {
        pt.name = n.to_string();
    } else {
        pt.name = format!("PivotTable{id}");
    }
    pt.source = cache.source.clone();
    let Some(loc) = x.child("location").and_then(|l| l.attr("ref")).and_then(|r| RangeRef::parse(&r.replace('$', ""))) else {
        cx.warn(format!("PivotTable \"{}\" has no valid location; kept as plain cells", pt.name));
        return None;
    };
    let pfs: Vec<&El> = x.child("pivotFields").map(|p| p.kids("pivotField").collect()).unwrap_or_default();
    let pf = |i: usize| pfs.get(i).copied();
    let idx_list = |name: &str, attr: &str| -> Vec<i64> {
        x.child(name).map(|e| e.children.iter().filter_map(|f| f.attr_i64(attr)).collect()).unwrap_or_default()
    };
    let row_x = idx_list("rowFields", "x");
    let col_x = idx_list("colFields", "x");
    let pages: Vec<&El> = x.child("pageFields").map(|p| p.kids("pageField").collect()).unwrap_or_default();

    // Layout from the first axis field (pivotField defaults: compact and outline on).
    let first_axis = row_x.iter().chain(col_x.iter()).find(|i| **i >= 0).and_then(|i| pf(*i as usize));
    let (compact, outline) = match first_axis {
        Some(f) => (f.flag("compact", true), f.flag("outline", true)),
        None => (x.flag("compact", true), x.flag("outline", false)),
    };
    pt.layout = if compact {
        PivotLayout::Compact
    } else if outline {
        PivotLayout::Outline
    } else {
        PivotLayout::Tabular
    };
    if let Some(f) = first_axis {
        pt.subtotals = f.flag("defaultSubtotal", true);
        pt.subtotals_top = f.flag("subtotalTop", true);
    }
    pt.grand_totals_rows = x.flag("colGrandTotals", true);
    pt.grand_totals_cols = x.flag("rowGrandTotals", true);
    pt.show_headers = x.flag("showHeaders", true);
    pt.style = x.child("pivotTableStyleInfo").and_then(|s| s.attr("name")).unwrap_or("").to_string();
    if x.path(&["filters", "filter"]).is_some() {
        cx.warn(format!("PivotTable \"{}\": label/value filters are not supported", pt.name));
    }

    let mut filters: Vec<PivotFilter> = Vec::new();

    // (fields, position of Σ Values (`-2`) among them)
    let axis = |cx: &mut Ctx<'_>, list: &[i64], filters: &mut Vec<PivotFilter>| -> (Vec<PivotField>, Option<usize>) {
        let mut v = Vec::new();
        let mut values_at = None;
        for &i in list {
            if i == -2 {
                values_at = Some(v.len());
                continue;
            }
            if i < 0 {
                continue;
            }
            let i = i as usize;
            let Some((name, group, number)) = field_ref(cx, cache, i) else { continue };
            let labels = cache.fields.get(i).map(|f| f.labels().to_vec()).unwrap_or_default();
            let mut f =
                PivotField { source_col: name.clone(), date_group: group, number_group: number, sort: PivotSort::None, collapsed_items: vec![] };
            if let Some(e) = pf(i) {
                f.sort = match e.attr("sortType") {
                    Some("ascending") => PivotSort::Asc,
                    Some("descending") => PivotSort::Desc,
                    _ => PivotSort::None,
                };
                let items = items_of(e);
                let lab = |it: &El| it.attr_u32("x").and_then(|k| labels.get(k as usize)).cloned();
                f.collapsed_items = items.iter().filter(|it| !it.flag("sd", true)).filter_map(|it| lab(it)).collect();
                if items.iter().any(|it| it.flag("h", false)) {
                    let shown: Vec<String> = items.iter().filter(|it| !it.flag("h", false)).filter_map(|it| lab(it)).collect();
                    if !filters.iter().any(|ff| same(&ff.source_col, &name)) {
                        filters.push(PivotFilter { source_col: name.clone(), selected: Some(shown), area: PivotFilterArea::Axis });
                    }
                }
            }
            v.push(f);
        }
        (v, values_at)
    };
    let mut page_filters = Vec::new();
    for p in &pages {
        let Some(i) = p.attr_i64("fld").filter(|i| *i >= 0).map(|i| i as usize) else { continue };
        let Some((name, _, _)) = field_ref(cx, cache, i) else { continue };
        let labels = cache.fields.get(i).map(|f| f.labels().to_vec()).unwrap_or_default();
        let selected = if let Some(k) = p.attr_u32("item") {
            labels.get(k as usize).map(|l| vec![l.clone()])
        } else {
            pf(i).and_then(|e| {
                let items = items_of(e);
                items.iter().any(|it| it.flag("h", false)).then(|| {
                    items
                        .iter()
                        .filter(|it| !it.flag("h", false))
                        .filter_map(|it| it.attr_u32("x").and_then(|k| labels.get(k as usize)).cloned())
                        .collect()
                })
            })
        };
        page_filters.push(PivotFilter { source_col: name, selected, area: PivotFilterArea::Page });
    }
    let mut axis_filters = Vec::new();
    let (rows, rows_values) = axis(cx, &row_x, &mut axis_filters);
    let (columns, cols_values) = axis(cx, &col_x, &mut axis_filters);
    pt.rows = rows;
    pt.columns = columns;
    // Σ Values: where its `-2` entry is, else `dataOnRows` and `dataPosition`; after the last
    // field by default. In Columns it always comes last, as the column layout puts it there.
    pt.values_on_rows = rows_values.is_some() || (cols_values.is_none() && x.flag("dataOnRows", false));
    pt.values_position =
        if pt.values_on_rows { rows_values.or_else(|| x.attr_u32("dataPosition").map(|p| p as usize)).filter(|p| *p < pt.rows.len()) } else { None };
    filters.extend(page_filters);
    filters.extend(axis_filters);
    pt.filters = filters;

    // Values.
    if let Some(df) = x.child("dataFields") {
        for d in df.kids("dataField") {
            let Some(i) = d.attr_u32("fld").map(|i| i as usize) else { continue };
            let Some(f) = cache.fields.get(i) else { continue };
            let func = match d.attr("subtotal") {
                None => PivotFunc::Sum,
                Some(s) => func_from(s).unwrap_or_else(|| {
                    cx.warn(format!("PivotTable function {s} is not supported; using Sum"));
                    PivotFunc::Sum
                }),
            };
            let ext = d
                .child("extLst")
                .into_iter()
                .flat_map(|l| l.kids("ext"))
                .filter_map(|e| e.child("dataField"))
                .find_map(|e| e.attr("pivotShowAs").map(str::to_string));
            let show_as = match (ext.as_deref(), d.attr("showDataAs")) {
                (Some("rankDescending"), _) => PivotShowAs::Rank,
                (Some(other), _) => {
                    cx.warn(format!("PivotTable show-values-as {other} is not supported"));
                    PivotShowAs::Normal
                }
                (None, None | Some("normal")) => PivotShowAs::Normal,
                (None, Some("percentOfTotal")) => PivotShowAs::PercentOfGrandTotal,
                (None, Some("percentOfCol")) => PivotShowAs::PercentOfColumnTotal,
                (None, Some("percentOfRow")) => PivotShowAs::PercentOfRowTotal,
                (None, Some("runTotal")) => PivotShowAs::RunningTotal,
                (None, Some(other)) => {
                    cx.warn(format!("PivotTable show-values-as {other} is not supported"));
                    PivotShowAs::Normal
                }
            };
            let number_format = d
                .attr_u32("numFmtId")
                .filter(|id| *id != 0)
                .and_then(|id| crate::tables::builtin_format(id).map(str::to_string).or_else(|| cx.num_fmts.get(&id).cloned()));
            let name = d.attr("name").map(str::to_string).unwrap_or_else(|| format!("{} of {}", func_caption(func), f.name));
            pt.values.push(PivotValue { source_col: f.name.clone(), func, name, show_as, number_format });
        }
    }

    // Calculated fields of the cache (each PivotTable on it can use them).
    for f in &cache.fields {
        if let Some(formula) = &f.formula
            && !f.name.trim().is_empty()
            && !pt.calculated_fields.iter().any(|c| same(&c.name, &f.name))
        {
            let formula = formula.trim();
            let formula = if formula.starts_with('=') { formula.to_string() } else { format!("={formula}") };
            pt.calculated_fields.push(PivotCalcField { name: f.name.clone(), formula });
        }
    }

    // Calculated items: an item of a row or column field, computed from that field's own items.
    for ci in &cache.calc_items {
        let Some(f) = cache.fields.get(ci.field) else {
            cx.warn(format!(
                "PivotTable \"{}\": a calculated item names cache field {} of {}; it was dropped",
                pt.name,
                ci.field,
                cache.fields.len()
            ));
            continue;
        };
        let Some((_, name)) = f.calc.iter().find(|(i, _)| *i as usize == ci.item) else {
            cx.warn(format!(
                "PivotTable \"{}\": a calculated item points at item {} of \"{}\", which isn't a calculated item; it was dropped",
                pt.name, ci.item, f.name
            ));
            continue;
        };
        if !pt.rows.iter().chain(pt.columns.iter()).any(|p| same(&p.source_col, &f.name)) {
            cx.warn(format!(
                "PivotTable \"{}\": the calculated item \"{name}\" is on \"{}\", which is not in Rows or Columns; it was dropped",
                pt.name, f.name
            ));
            continue;
        }
        if f.group.is_some() {
            cx.warn(format!("PivotTable \"{}\": the calculated item \"{name}\" is on the grouped field \"{}\"; it was dropped", pt.name, f.name));
            continue;
        }
        let Ok(compiled) = pivot_item::compile(&f.name, &ci.formula) else {
            cx.warn(format!("PivotTable \"{}\": the calculated item \"{name}\" has a formula this version can't read; it was dropped", pt.name));
            continue;
        };
        pt.calculated_items.push(PivotCalcItem { field: f.name.clone(), name: name.clone(), formula: compiled.dialog_form(&f.name) });
    }

    pt.anchor = loc.start;
    let above = if pages.is_empty() { 0 } else { pages.len() as u32 + 1 };
    pt.last_range = Some(RangeRef::new(CellRef::new(loc.start.row.saturating_sub(above), loc.start.col), loc.end));
    Some(pt)
}
