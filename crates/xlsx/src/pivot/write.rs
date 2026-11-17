//! Writing PivotTables: one cache per table, with a records snapshot of the source list.

use super::*;

#[derive(Default)]
struct Stats {
    blank: bool,
    text: bool,
    number: bool,
    non_int: bool,
    boolean: bool,
    error: bool,
    long: bool,
    min: f64,
    max: f64,
}
impl Stats {
    fn add(&mut self, v: &Value) {
        match v {
            Value::Empty => self.blank = true,
            Value::Number(n) => {
                if !self.number {
                    self.min = *n;
                    self.max = *n;
                }
                self.number = true;
                self.min = self.min.min(*n);
                self.max = self.max.max(*n);
                if n.fract() != 0.0 {
                    self.non_int = true;
                }
            }
            Value::Text(t) => {
                self.text = true;
                if t.chars().count() > 255 {
                    self.long = true;
                }
            }
            Value::Bool(_) => self.boolean = true,
            Value::Error(_) => self.error = true,
            Value::Array(_) => self.add(&v.scalar()),
        }
    }
    /// Excel counts booleans and errors as strings in the cache field flags.
    fn strings(&self) -> bool {
        self.text || self.boolean || self.error
    }
    /// Kinds for `containsMixedTypes`: strings (text, booleans, errors) and numbers.
    fn kinds(&self) -> usize {
        usize::from(self.strings()) + usize::from(self.number)
    }
}
/// Grouping of a cache field: a date part or a numeric range.
pub(crate) struct Group {
    by: PivotDateGroup,
    /// The numeric range (`rangePr`'s `startNum`/`endNum`/`groupInterval`), when it is a number
    /// group; `by` is then `None` and `groupBy="range"`.
    number: Option<PivotNumberGroup>,
    /// The database field the grouping applies to.
    base: usize,
    /// The next (coarser) grouping field of the same base.
    par: Option<usize>,
    start: f64,
    end: f64,
    /// Group item captions, including the `<start` and `>end` items.
    items: Vec<String>,
}
struct CField {
    name: String,
    /// Column offset in the source (database fields only).
    col: Option<u32>,
    stats: Stats,
    /// Shared items (written when `itemized`).
    items: Vec<Value>,
    index: HashMap<Key, u32>,
    itemized: bool,
    group: Option<Group>,
    /// A calculated field's formula (without `=`); such fields aren't database fields.
    formula: Option<String>,
    /// Calculated items: appended to the shared items with `f="1"` (Excel's marker) and listed
    /// as `<item f="1" x>` in the table.
    calc_items: Vec<CalcOut>,
}
/// A calculated item written with its field.
struct CalcOut {
    name: String,
    /// Formula in the dialog form (`=North+South`).
    formula: String,
}
impl CField {
    /// Item captions the PivotTable's `<item x>` indices refer to (real items, then calculated
    /// ones).
    fn item_labels(&self) -> Vec<String> {
        match &self.group {
            Some(g) => g.items.clone(),
            None => self.items.iter().map(label).chain(self.calc_items.iter().map(|c| c.name.clone())).collect(),
        }
    }
}
/// A cache built from a PivotTable's source.
struct Cache {
    fields: Vec<CField>,
    /// Pivot field → cache field, by the pivot field's position in rows, then columns.
    axis_map: Vec<Option<usize>>,
    source_xml: String,
    records: Option<(u64, String)>,
}
/// PivotTables being written; the tables themselves are finished after the styles part (their
/// value number formats need style number-format ids).
#[derive(Default)]
pub struct PivotWriter {
    /// (cache id, workbook-relative part name).
    caches: Vec<(u32, String)>,
    pending: Vec<Pending>,
    tables: u32,
    /// The cache ids the file's own reports keep (see [`Self::add_read_only`]): their parts are
    /// written as they came, so a report Cella computes must not take the same id.
    taken_cache_ids: HashSet<u32>,
    /// Whether the ids those reports keep have been collected from the whole workbook yet.
    reserved: bool,
}
struct Pending {
    part: String,
    /// The PivotTable part's relationships, written with it (they hold its slicer cache).
    rels_part: String,
    sheet: usize,
    idx: usize,
    cache_id: u32,
    cache: Cache,
}
impl PivotWriter {
    /// Writes the caches of a sheet's PivotTables and adds the sheet's relationships to its
    /// PivotTable parts. Pivots whose source can't be resolved are not written.
    pub fn add_sheet(&mut self, wb: &Workbook, si: usize, out: &mut Out, rels: &mut Rels, slicers: &crate::slicer::SlicerWriter) {
        let Some(sheet) = wb.sheet(si) else { return };
        if !self.reserved {
            self.reserved = true;
            self.reserve_read_only(wb);
        }
        for (idx, pt) in sheet.pivots.iter().enumerate() {
            // A report Cella does not compute is carried, not built: its parts come back as the
            // file had them (the cache id it names included), and the cells under it are its
            // cached values, which nothing here touches.
            if pt.is_read_only() {
                self.add_read_only(sheet, pt, out, rels);
                continue;
            }
            // Fields a modelled slicer shows are itemized in the cache so its `<i x>` can point at
            // them; the slicer's field is checked before the sheet loop (`SlicerWriter::new`).
            let fields = slicers.fields_for_pivot(&pt.name);
            let Some(cache) = build_cache(wb, pt, si, &fields) else { continue };
            let n = self.next_cache_id();
            let cache_part = format!("xl/pivotCache/pivotCacheDefinition{n}.xml");
            let mut cr = Rels::default();
            let rec_rid = if let Some((count, xml)) = &cache.records {
                let rid = cr.add("pivotCacheRecords", &format!("pivotCacheRecords{n}.xml"));
                let s = format!("{XML_DECL}<pivotCacheRecords xmlns=\"{NS_MAIN}\" xmlns:r=\"{NS_REL}\" count=\"{count}\">{xml}</pivotCacheRecords>");
                out.part(&format!("xl/pivotCache/pivotCacheRecords{n}.xml"), Some(CT_PIVOT_CACHE_REC), s.into_bytes());
                Some(rid)
            } else {
                None
            };
            // What the source cache definition kept that the model has no room for.
            let key = crate::passthrough::pivot_cache_key(&pt.name);
            let ext = crate::passthrough::owner_extras(sheet.preserved.as_deref(), key.as_deref(), &mut cr, out);
            out.part(&cache_part, Some(CT_PIVOT_CACHE_DEF), cache_xml(&cache, n, rec_rid.as_deref(), wb.date_system, &ext).into_bytes());
            if !cr.is_empty() {
                out.part(&format!("xl/pivotCache/_rels/pivotCacheDefinition{n}.xml.rels"), None, cr.xml().into_bytes());
            }
            self.caches.push((n, format!("pivotCache/pivotCacheDefinition{n}.xml")));
            self.tables += 1;
            let t = self.tables;
            rels.add("pivotTable", &format!("../pivotTables/pivotTable{t}.xml"));
            self.pending.push(Pending {
                part: format!("xl/pivotTables/pivotTable{t}.xml"),
                rels_part: format!("xl/pivotTables/_rels/pivotTable{t}.xml.rels"),
                sheet: si,
                idx,
                cache_id: n,
                cache,
            });
        }
    }

    /// The cache id to write a report Cella computes' cache under: the lowest one no other
    /// report of the package took (a carried report keeps the id the file gave it).
    fn next_cache_id(&mut self) -> u32 {
        let taken = |id: u32, caches: &[(u32, String)]| caches.iter().any(|(c, _)| *c == id);
        let mut n = 1u32;
        while self.taken_cache_ids.contains(&n) || taken(n, &self.caches) {
            if n == u32::MAX {
                break;
            }
            n = n.saturating_add(1);
        }
        self.taken_cache_ids.insert(n);
        n
    }

    /// Records the cache ids the workbook's carried reports keep, before anything is written: the
    /// reports come in sheet order, and one of them can be on a later sheet than a report Cella
    /// computes.
    fn reserve_read_only(&mut self, wb: &Workbook) {
        for sheet in &wb.sheets {
            for pt in sheet.pivots.iter().filter(|p| p.is_read_only()) {
                if let Some((table, _)) = kept_read_only(sheet, &pt.name) {
                    self.taken_cache_ids.insert(cache_id_of(&table));
                }
            }
        }
    }

    /// Writes a PivotTable Cella does not compute: its own part, its cache definition, its
    /// records and the data model behind them, exactly as the file had them. Only the part names
    /// are ours (the report keeps its `pivotTableN.xml` numbering and its cache the id it names),
    /// and its `cacheId` therefore still points at its own cache. Nothing is recomputed and
    /// nothing is repaired: the report is a document, not a query.
    fn add_read_only(&mut self, sheet: &cella_model::Sheet, pt: &PivotTable, out: &mut Out, rels: &mut Rels) {
        let Some((table, cache)) = kept_read_only(sheet, &pt.name) else { return };
        let Some(key) = crate::passthrough::pivot_key(&pt.name) else { return };
        let cache_id = cache_id_of(&table);
        let t = self.tables.saturating_add(1);
        // The report part, its cache and the cache's records take names of our own (the report's
        // `cacheId` is in its bytes, so the cache keeps that number); every other part of the
        // graph — the data model above all — keeps the name it came with.
        let same = |a: &str, b: &str| crate::package::normalize(a) == crate::package::normalize(b);
        let target_of = |part: &cella_model::PreservedPart, kind: &str| {
            part.rels.iter().find(|r| r.rel_type.rsplit('/').next() == Some(kind) && !r.external).map(|r| r.target.clone())
        };
        let records = target_of(&cache, "pivotCacheRecords");
        let name_of = |q: &cella_model::PreservedPart| -> String {
            if same(&q.name, &table.name) {
                format!("xl/pivotTables/pivotTable{t}.xml")
            } else if same(&q.name, &cache.name) {
                format!("xl/pivotCache/pivotCacheDefinition{cache_id}.xml")
            } else if records.as_deref().is_some_and(|r| same(&q.name, r)) {
                format!("xl/pivotCache/pivotCacheRecords{cache_id}.xml")
            } else {
                q.name.clone()
            }
        };
        let (_, names) = crate::passthrough::write_pivot_parts(sheet.preserved.as_deref(), Some(&key), out, &name_of);
        let written = |original: &str| names.get(&crate::package::normalize(original)).cloned();
        let (Some(table_part), Some(cache_part)) = (written(&table.name), written(&cache.name)) else { return };
        self.tables = t;
        self.taken_cache_ids.insert(cache_id);
        let target = table_part.strip_prefix("xl/").unwrap_or(&table_part);
        rels.add("pivotTable", &format!("../{target}"));
        self.caches.push((cache_id, cache_part.strip_prefix("xl/").unwrap_or(&cache_part).to_string()));
    }

    /// Adds the workbook relationships to the caches; returns the `<pivotCaches>` element (empty
    /// when there are none).
    pub fn workbook_xml(&self, wb_rels: &mut Rels) -> String {
        if self.caches.is_empty() {
            return String::new();
        }
        let mut s = String::from("<pivotCaches>");
        for (id, target) in &self.caches {
            let rid = wb_rels.add("pivotCacheDefinition", target);
            let _ = write!(s, "<pivotCache cacheId=\"{id}\" r:id=\"{rid}\"/>");
        }
        s.push_str("</pivotCaches>");
        s
    }

    /// Number formats used by value fields (to be registered in the styles part).
    pub fn number_formats(&self, wb: &Workbook) -> Vec<String> {
        let mut v = Vec::new();
        for p in &self.pending {
            if let Some(pt) = wb.sheet(p.sheet).and_then(|s| s.pivots.get(p.idx)) {
                for val in &pt.values {
                    if let Some(f) = &val.number_format
                        && !v.contains(f)
                    {
                        v.push(f.clone());
                    }
                }
            }
        }
        v
    }

    /// The `cacheId` of the cache written for the PivotTable `pivot` (case-insensitive name), or
    /// `None` when that PivotTable's cache is not written (its source can't be resolved).
    pub fn cache_id(&self, wb: &Workbook, pivot: &str) -> Option<u32> {
        self.pending.iter().find(|p| wb.sheet(p.sheet).and_then(|s| s.pivots.get(p.idx)).is_some_and(|pt| same(&pt.name, pivot))).map(|p| p.cache_id)
    }

    /// The shared items written for the cache `cache_id`'s field (in the order `<i x>` indexes
    /// them). `None` when the field is gone or was not itemized.
    pub fn field_items(&self, cache_id: u32, field: &str) -> Option<Vec<Value>> {
        let p = self.pending.iter().find(|p| p.cache_id == cache_id)?;
        let f = p.cache.fields.iter().find(|f| f.col.is_some() && same(&f.name, field) && f.itemized)?;
        Some(f.items.clone())
    }

    /// Writes the PivotTable parts. `fmt_id` maps a number format code to its styles id; `slicers`
    /// names the modelled slicer caches that filter each PivotTable, written as its extension and
    /// its own relationship (the same contract as the kept path: the source extension was dropped
    /// on read when the cache was modelled, so exactly one is written).
    pub fn finish(self, wb: &Workbook, out: &mut Out, fmt_id: &dyn Fn(&str) -> Option<u32>, slicers: &[crate::slicer::CacheLink]) {
        for p in self.pending {
            let Some(pt) = wb.sheet(p.sheet).and_then(|s| s.pivots.get(p.idx)) else { continue };
            let mut tr = Rels::default();
            tr.add("pivotCacheDefinition", &format!("../pivotCache/pivotCacheDefinition{}.xml", p.cache_id));
            // The extensions the source PivotTable kept that the model has no room for (the slicer
            // cache reference among them), and the parts they name.
            let key = crate::passthrough::pivot_key(&pt.name);
            let ext = crate::passthrough::owner_extras(wb.sheet(p.sheet).and_then(|s| s.preserved.as_deref()), key.as_deref(), &mut tr, out);
            let model_ext = crate::slicer::pivot_ext(&pt.name, slicers, &mut tr);
            let xml = table_xml(pt, p.idx, p.cache_id, &p.cache, fmt_id, &format!("{model_ext}{ext}"));
            out.part(&p.part, Some(CT_PIVOT_TABLE), xml.into_bytes());
            out.part(&p.rels_part, None, tr.xml().into_bytes());
        }
    }
}
/// The kept parts of a carried report: its own pivotTable part and its cache definition part, as
/// the pass-through kept them. `None` when the file had nothing to carry (a report Cella
/// created, or one whose parts were dropped on read).
fn kept_read_only(sheet: &cella_model::Sheet, name: &str) -> Option<(cella_model::PreservedPart, cella_model::PreservedPart)> {
    let key = crate::passthrough::pivot_key(name)?;
    let (parts, sources) = crate::passthrough::owner_kept(sheet.preserved.as_deref(), Some(&key));
    let find = |kind: &str| -> Option<cella_model::PreservedPart> {
        let r = sources.iter().find(|r| r.rel_type.rsplit('/').next() == Some(kind) && !r.external)?;
        parts.iter().find(|q| crate::package::normalize(&q.name) == crate::package::normalize(&r.target)).cloned()
    };
    let table = find("pivotTable")?;
    // The cache definition is the report part's own relationship (the one its `cacheId` means).
    let target = table.rels.iter().find(|r| r.rel_type.rsplit('/').next() == Some("pivotCacheDefinition") && !r.external)?;
    let cache = parts.iter().find(|q| crate::package::normalize(&q.name) == crate::package::normalize(&target.target)).cloned()?;
    Some((table, cache))
}
/// The `cacheId` a carried report part names: the id its cache is written under, which the report
/// part keeps byte for byte.
fn cache_id_of(table: &cella_model::PreservedPart) -> u32 {
    crate::xml::parse(&table.data).ok().and_then(|x| x.attr_u32("cacheId")).unwrap_or(0)
}
/// Resolves a pivot source to (sheet, range including the header row, worksheetSource XML).
fn resolve_source(wb: &Workbook, text: &str, default_sheet: usize, depth: u32) -> Option<(usize, RangeRef, String)> {
    let t = text.trim().trim_start_matches('=').trim();
    if t.is_empty() || depth > 4 {
        return None;
    }
    if let Some((si, ti)) = wb.table(t)
        && let Some(sh) = wb.sheet(si)
        && let Some(tb) = sh.tables.get(ti)
    {
        if !tb.header_row {
            return None;
        }
        let end_row = tb.range.end.row.saturating_sub(tb.totals_row as u32).max(tb.range.start.row);
        let r = RangeRef::new(tb.range.start, CellRef::new(end_row, tb.range.end.col));
        // Table names are sanitized when written; refer by range when this one would change.
        let valid = !tb.name.is_empty()
            && tb.name.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '.')
            && tb.name.starts_with(|c: char| c.is_alphabetic() || c == '_')
            && CellRef::parse(&tb.name).is_none();
        let xml = if valid { format!("<worksheetSource name=\"{}\"/>", esc_attr(&tb.name)) } else { range_source_xml(&sh.name, r) };
        return Some((si, r, xml));
    }
    let (sheet, body) = cella_core::split_sheet(t);
    if let Some(r) = RangeRef::parse(body) {
        let si = match &sheet {
            Some(n) => wb.sheet_index(n)?,
            None => default_sheet,
        };
        let sh = wb.sheet(si)?;
        let r = if r.is_full_cols() || r.is_full_rows() { r.intersection(&sh.used_range()?)? } else { r };
        return Some((si, r, range_source_xml(&sh.name, r)));
    }
    if sheet.is_none()
        && let Some(n) = wb.name(t, default_sheet)
    {
        let (si, r, _) = resolve_source(wb, &n.formula.clone(), default_sheet, depth + 1)?;
        let xml = if n.scope.is_none() {
            format!("<worksheetSource name=\"{}\"/>", esc_attr(&n.name))
        } else {
            wb.sheet(si).map(|s| range_source_xml(&s.name, r))?
        };
        return Some((si, r, xml));
    }
    None
}
fn range_source_xml(sheet: &str, r: RangeRef) -> String {
    let name: String = sheet.chars().take(31).collect();
    format!("<worksheetSource ref=\"{}\" sheet=\"{}\"/>", r.a1(), esc_attr(&name))
}
fn header_text(v: &Value) -> String {
    match v {
        Value::Empty => String::new(),
        v => label(v).trim().to_string(),
    }
}
/// The cache field names a source range gets: its header row, with empty headers numbered
/// (`ColumnN`) and repeats suffixed (`Region2`), exactly as `build_cache` writes them. Also used
/// to check a slicer's field before the pivot cache is built.
fn header_names(sh: &cella_model::Sheet, r: RangeRef) -> Vec<String> {
    let mut names = Vec::new();
    // Lower-case names taken, and the next suffix for each header (linear for repeated headers).
    let mut taken = std::collections::HashSet::new();
    let mut next_suffix: HashMap<String, usize> = HashMap::new();
    for (k, c) in (r.start.col..=r.end.col).enumerate() {
        let mut h = header_text(&sh.value(CellRef::new(r.start.row, c)).scalar());
        if h.is_empty() {
            h = format!("Column{}", k + 1);
        }
        let mut n = h.clone();
        let mut i = next_suffix.get(&h.to_lowercase()).copied().unwrap_or(2);
        while taken.contains(&n.to_lowercase()) {
            n = format!("{h}{i}");
            i += 1;
        }
        next_suffix.insert(h.to_lowercase(), i);
        taken.insert(n.to_lowercase());
        names.push(n);
    }
    names
}
/// The field names a PivotTable's source would get in a written cache, and the source sheet, when
/// the cache can be written at all (`build_cache` fails on the same conditions). Lets a slicer be
/// checked before the sheet loop, where the pivot cache does not exist yet.
pub fn source_fields(wb: &Workbook, pt: &PivotTable, default_sheet: usize) -> Option<(usize, Vec<String>)> {
    let (si, r, _) = resolve_source(wb, &pt.source, default_sheet, 0)?;
    if r.count() > MAX_SOURCE_CELLS {
        return None;
    }
    Some((si, header_names(wb.sheet(si)?, r)))
}
/// The source sheet, range and absolute column of one field of a PivotTable's source, when the
/// cache can be written at all. A timeline reads its date bounds from that column.
pub fn source_column(wb: &Workbook, pt: &PivotTable, default_sheet: usize, field: &str) -> Option<(usize, RangeRef, u32)> {
    let (si, r, _) = resolve_source(wb, &pt.source, default_sheet, 0)?;
    if r.count() > MAX_SOURCE_CELLS {
        return None;
    }
    let names = header_names(wb.sheet(si)?, r);
    let col = names.iter().position(|n| n.eq_ignore_ascii_case(field))?;
    Some((si, r, r.start.col.saturating_add(col as u32)))
}
fn build_cache(wb: &Workbook, pt: &PivotTable, pivot_sheet: usize, slicer_fields: &[String]) -> Option<Cache> {
    let (si, r, source_xml) = resolve_source(wb, &pt.source, pivot_sheet, 0)?;
    if r.count() > MAX_SOURCE_CELLS {
        return None;
    }
    let sh = wb.sheet(si)?;
    let mut fields: Vec<CField> = Vec::new();
    for (k, n) in header_names(sh, r).into_iter().enumerate() {
        fields.push(CField {
            name: n,
            col: Some(k as u32),
            stats: Stats::default(),
            items: vec![],
            index: HashMap::new(),
            itemized: false,
            group: None,
            formula: None,
            calc_items: vec![],
        });
    }
    let find = |fields: &[CField], name: &str| fields.iter().position(|f| f.col.is_some() && same(&f.name, name));

    // Records (rows that aren't entirely blank), read once.
    let mut rows: Vec<u32> = Vec::new();
    if r.end.row > r.start.row {
        for row in r.start.row + 1..=r.end.row {
            let blank = (r.start.col..=r.end.col).all(|c| sh.value(CellRef::new(row, c)).scalar().is_empty());
            if !blank {
                rows.push(row);
            }
        }
    }
    for &row in &rows {
        for f in fields.iter_mut() {
            let c = r.start.col + f.col.unwrap_or(0);
            f.stats.add(&sh.value(CellRef::new(row, c)).scalar());
        }
    }

    // Which fields are on an axis, and how dates are grouped: per base column, the base field
    // takes the finest grouping and coarser ones become extra (non-database) fields chained
    // through `par`, as Excel lays them out.
    let axis: Vec<&PivotField> = pt.rows.iter().chain(pt.columns.iter()).collect();
    let groupable = |f: &CField| {
        let s = &f.stats;
        s.number
            && !s.text
            && !s.boolean
            && !s.error
            && datetime_from_serial(wb.date_system, s.min).is_some()
            && datetime_from_serial(wb.date_system, s.max).is_some()
    };
    // (base column field, groupings fine → coarse)
    let mut groupings: Vec<(usize, Vec<PivotDateGroup>)> = Vec::new();
    for pf in &axis {
        let Some(b) = find(&fields, &pf.source_col) else { continue };
        if let Some(ng) = pf.number_group.filter(PivotNumberGroup::is_valid) {
            let (min, max) = fields.get(b).map(|f| (f.stats.min, f.stats.max)).unwrap_or((0.0, 0.0));
            // Excel writes a grouped number field's base values into `sharedItems` (`count` + one
            // `<n>` each). Without them it treats the grouping as unreliable, refreshes the cache
            // and re-anchors the buckets at the field's minimum, which can silently change the
            // report's labels; with them it keeps the `rangePr` bounds and the captions.
            let mut vals: Vec<Value> = Vec::new();
            let mut seen: HashSet<Key> = HashSet::new();
            if let Some(col) = fields.get(b).and_then(|f| f.col) {
                for &row in &rows {
                    let v = sh.value(CellRef::new(row, r.start.col + col)).scalar();
                    if !v.is_empty() && seen.insert(key(&v)) && vals.len() < MAX_ITEMS {
                        vals.push(v);
                    }
                }
            }
            if let Some(f) = fields.get_mut(b) {
                f.items = vals;
                f.group = Some(Group { by: PivotDateGroup::None, number: Some(ng), base: b, par: None, start: min, end: max, items: ng.captions() });
            }
            continue;
        }
        if pf.date_group == PivotDateGroup::None || !fields.get(b).is_some_and(groupable) {
            continue;
        }
        match groupings.iter_mut().find(|(x, _)| *x == b) {
            Some((_, v)) => {
                if !v.contains(&pf.date_group) {
                    v.push(pf.date_group);
                }
            }
            None => groupings.push((b, vec![pf.date_group])),
        }
    }
    let fineness = |g: &PivotDateGroup| match g {
        PivotDateGroup::Days => 0,
        PivotDateGroup::Months => 1,
        PivotDateGroup::Quarters => 2,
        PivotDateGroup::Years => 3,
        PivotDateGroup::None => 4,
    };
    for (b, list) in groupings.iter_mut() {
        list.sort_by_key(fineness);
        let (min, max) = fields.get(*b).map(|f| (f.stats.min, f.stats.max)).unwrap_or((0.0, 0.0));
        let (start, end) = (min.floor(), max.floor() + 1.0);
        let make =
            |by: PivotDateGroup| Group { by, number: None, base: *b, par: None, start, end, items: group_items(wb.date_system, by, start, end) };
        let mut prev = *b;
        for (k, by) in list.iter().enumerate() {
            if k == 0 {
                if let Some(f) = fields.get_mut(*b) {
                    f.group = Some(make(*by));
                }
                continue;
            }
            let new = fields.len();
            let caption = group_caption(*by);
            let mut n = caption.to_string();
            let mut i = 2;
            while fields.iter().any(|f| same(&f.name, &n)) {
                n = format!("{caption}{i}");
                i += 1;
            }
            if let Some(g) = fields.get_mut(prev).and_then(|f| f.group.as_mut()) {
                g.par = Some(new);
            }
            fields.push(CField {
                name: n,
                col: None,
                stats: Stats::default(),
                items: vec![],
                index: HashMap::new(),
                itemized: false,
                group: Some(make(*by)),
                formula: None,
                calc_items: vec![],
            });
            prev = new;
        }
    }
    let mut axis_map: Vec<Option<usize>> = Vec::with_capacity(axis.len());
    for pf in &axis {
        let Some(b) = find(&fields, &pf.source_col) else {
            axis_map.push(None);
            continue;
        };
        if pf.number_group.filter(PivotNumberGroup::is_valid).is_some() {
            axis_map.push(Some(b));
            continue;
        }
        let grouped = fields.iter().position(|f| f.group.as_ref().is_some_and(|g| g.number.is_none() && g.base == b && g.by == pf.date_group));
        match grouped {
            Some(i) if pf.date_group != PivotDateGroup::None => axis_map.push(Some(i)),
            _ => {
                if let Some(f) = fields.get_mut(b)
                    && f.group.is_none()
                {
                    f.itemized = true;
                }
                axis_map.push(Some(b));
            }
        }
    }
    // Page fields (filters not on an axis) need items too.
    for f in &pt.filters {
        let on_axis = axis.iter().any(|a| same(&a.source_col, &f.source_col));
        if !on_axis
            && let Some(i) = find(&fields, &f.source_col)
            && let Some(cf) = fields.get_mut(i)
        {
            cf.itemized = true;
        }
    }
    // A field a modelled slicer shows needs its values itemized too, even when it is in no area:
    // the slicer cache's `<i x>` indexes this cache's shared items.
    for name in slicer_fields {
        if let Some(i) = find(&fields, name)
            && let Some(cf) = fields.get_mut(i)
            && cf.group.is_none()
        {
            cf.itemized = true;
        }
    }
    for f in fields.iter_mut() {
        if f.group.is_some() {
            f.itemized = false;
        } else if f.col.is_some() && (f.stats.text || f.stats.boolean || f.stats.error) {
            f.itemized = true;
        }
    }
    // Calculated items: an extra item of a field on an axis, after its real items. A grouped
    // field's items are ranges, so it cannot carry one (the engine drops them when it groups).
    for ci in pt.calculated_items.iter().take(MAX_CALC_ITEMS) {
        let Some(ai) = axis.iter().position(|f| same(&f.source_col, &ci.field)) else { continue };
        let Some(fi) = axis_map.get(ai).copied().flatten() else { continue };
        let Some(cf) = fields.get_mut(fi) else { continue };
        let name = ci.name.trim();
        if cf.group.is_some() || cf.col.is_none() || !cf.itemized || name.is_empty() {
            continue;
        }
        if cf.calc_items.iter().any(|c| same(&c.name, name)) {
            continue;
        }
        // Only a formula this model can write for this field is kept (the reader warns otherwise).
        let Ok(compiled) = pivot_item::compile(&cf.name, &ci.formula) else { continue };
        if compiled.items().is_empty() {
            continue;
        }
        cf.calc_items.push(CalcOut { name: name.to_string(), formula: ci.formula.clone() });
    }
    // Shared items, in order of first appearance.
    for &row in &rows {
        for f in fields.iter_mut().filter(|f| f.itemized) {
            let c = r.start.col + f.col.unwrap_or(0);
            let v = sh.value(CellRef::new(row, c)).scalar();
            let k = key(&v);
            if !f.index.contains_key(&k) && f.items.len() < MAX_ITEMS {
                f.index.insert(k, f.items.len() as u32);
                f.items.push(v);
            }
        }
    }
    // Records snapshot.
    let db_fields = fields.iter().filter(|f| f.col.is_some()).count() as u64;
    let records = if (rows.len() as u64).saturating_mul(db_fields) <= MAX_RECORD_CELLS {
        let mut s = String::new();
        for &row in &rows {
            s.push_str("<r>");
            for f in fields.iter().filter(|f| f.col.is_some()) {
                let c = r.start.col + f.col.unwrap_or(0);
                let v = sh.value(CellRef::new(row, c)).scalar();
                if f.itemized {
                    match f.index.get(&key(&v)) {
                        Some(i) => {
                            let _ = write!(s, "<x v=\"{i}\"/>");
                        }
                        None => s.push_str(&item_xml(&v, wb.date_system, false)),
                    }
                } else {
                    let dates = f.group.as_ref().is_some_and(|g| g.by != PivotDateGroup::None);
                    s.push_str(&item_xml(&v, wb.date_system, dates));
                }
            }
            s.push_str("</r>");
        }
        Some((rows.len() as u64, s))
    } else {
        None
    };
    // Calculated fields come after the database and grouping fields.
    for cf in &pt.calculated_fields {
        let f = cf.formula.trim();
        let f = f.strip_prefix('=').unwrap_or(f).trim();
        if cf.name.trim().is_empty() || f.is_empty() || fields.iter().any(|x| same(&x.name, &cf.name)) {
            continue;
        }
        fields.push(CField {
            name: cf.name.clone(),
            col: None,
            stats: Stats::default(),
            items: vec![],
            index: HashMap::new(),
            itemized: false,
            group: None,
            formula: Some(f.to_string()),
            calc_items: vec![],
        });
    }
    Some(Cache { fields, axis_map, source_xml, records })
}
fn group_items(sys: DateSystem, by: PivotDateGroup, start: f64, end: f64) -> Vec<String> {
    let mut v = vec![format!("<{}", mdy(sys, start))];
    match by {
        PivotDateGroup::Years => {
            let y0 = datetime_from_serial(sys, start).map(|d| d.year).unwrap_or(1900);
            let y1 = datetime_from_serial(sys, end - 1.0).map(|d| d.year).unwrap_or(y0).max(y0);
            for y in y0..=y1.min(y0 + 10_000) {
                v.push(y.to_string());
            }
        }
        PivotDateGroup::Quarters => v.extend((1..=4).map(|q| format!("Qtr{q}"))),
        PivotDateGroup::Months => v.extend(MONTHS.iter().map(|m| m.to_string())),
        PivotDateGroup::Days | PivotDateGroup::None => {
            const DAYS: [u32; 12] = [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
            for (m, n) in MONTHS.iter().zip(DAYS) {
                for d in 1..=n {
                    v.push(format!("{d}-{m}"));
                }
            }
        }
    }
    v.push(format!(">{}", mdy(sys, end)));
    v
}
/// A cache value element (`<n>`, `<s>`, `<m>`…); `date` writes numbers as `<d>`.
fn item_xml(v: &Value, sys: DateSystem, date: bool) -> String {
    match v {
        Value::Empty => "<m/>".into(),
        Value::Number(n) => {
            if date && let Some(d) = iso(sys, *n) {
                return format!("<d v=\"{d}\"/>");
            }
            format!("<n v=\"{}\"/>", crate::xml::num(*n))
        }
        Value::Text(t) => format!("<s v=\"{}\"/>", esc_attr(t)),
        Value::Bool(b) => format!("<b v=\"{}\"/>", u8::from(*b)),
        Value::Error(e) => format!("<e v=\"{}\"/>", esc_attr(e.as_str())),
        Value::Array(_) => item_xml(&v.scalar(), sys, date),
    }
}
fn cache_xml(c: &Cache, cache_id: u32, records_rid: Option<&str>, sys: DateSystem, ext: &str) -> String {
    let mut s = format!("{XML_DECL}<pivotCacheDefinition xmlns=\"{NS_MAIN}\" xmlns:r=\"{NS_REL}\"");
    match (records_rid, &c.records) {
        (Some(rid), Some((count, _))) => {
            let _ = write!(s, " r:id=\"{rid}\" recordCount=\"{count}\"");
        }
        _ => s.push_str(" saveData=\"0\""),
    }
    // No `refreshOnLoad`: with it Excel rebuilt the cache on every open and re-anchored a number
    // field's grouping at the data's minimum, changing the report's labels. Records are saved
    // (`r:id` above), so the cache is complete without a refresh.
    s.push_str(" refreshedBy=\"Cella\" createdVersion=\"6\" refreshedVersion=\"6\" minRefreshableVersion=\"3\">");
    let _ = write!(s, "<cacheSource type=\"worksheet\">{}</cacheSource>", c.source_xml);
    let _ = write!(s, "<cacheFields count=\"{}\">", c.fields.len());
    for f in &c.fields {
        let fmt = if f.group.as_ref().is_some_and(|g| g.by != PivotDateGroup::None) { 14 } else { 0 };
        let _ = write!(s, "<cacheField name=\"{}\" numFmtId=\"{fmt}\"", esc_attr(&f.name));
        if let Some(formula) = &f.formula {
            let _ = write!(s, " formula=\"{}\"", esc_attr(formula));
        }
        if f.col.is_none() {
            s.push_str(" databaseField=\"0\"");
        }
        s.push('>');
        let st = &f.stats;
        if f.col.is_some() {
            s.push_str("<sharedItems");
            if let Some(g) = &f.group
                && g.number.is_some()
            {
                s.push_str(" containsSemiMixedTypes=\"0\" containsString=\"0\" containsNumber=\"1\"");
                if !st.non_int {
                    s.push_str(" containsInteger=\"1\"");
                }
                if st.blank {
                    s.push_str(" containsBlank=\"1\"");
                }
                let _ = write!(s, " minValue=\"{}\" maxValue=\"{}\"", crate::xml::num(st.min), crate::xml::num(st.max));
            } else if let Some(g) = &f.group {
                s.push_str(" containsSemiMixedTypes=\"0\" containsNonDate=\"0\" containsDate=\"1\" containsString=\"0\"");
                if st.blank {
                    s.push_str(" containsBlank=\"1\"");
                }
                if let (Some(a), Some(b)) = (iso(sys, st.min), iso(sys, st.max)) {
                    let _ = write!(s, " minDate=\"{a}\" maxDate=\"{b}\"");
                }
                let _ = g;
            } else {
                if !st.strings() && !st.blank {
                    s.push_str(" containsSemiMixedTypes=\"0\"");
                }
                if !st.strings() {
                    s.push_str(" containsString=\"0\"");
                }
                if st.blank {
                    s.push_str(" containsBlank=\"1\"");
                }
                if st.kinds() > 1 {
                    s.push_str(" containsMixedTypes=\"1\"");
                }
                if st.number {
                    s.push_str(" containsNumber=\"1\"");
                    if !st.non_int {
                        s.push_str(" containsInteger=\"1\"");
                    }
                    let _ = write!(s, " minValue=\"{}\" maxValue=\"{}\"", crate::xml::num(st.min), crate::xml::num(st.max));
                }
                if st.long {
                    s.push_str(" longText=\"1\"");
                }
            }
            let number_group = f.group.as_ref().is_some_and(|g| g.number.is_some());
            if f.itemized || number_group {
                let _ = write!(s, " count=\"{}\">", f.items.len() + f.calc_items.len());
                for v in &f.items {
                    s.push_str(&item_xml(v, sys, false));
                }
                // A calculated item is the last shared item, marked `f="1"` (Excel's marker).
                for c in &f.calc_items {
                    let _ = write!(s, "<s v=\"{}\" f=\"1\"/>", esc_attr(&c.name));
                }
                s.push_str("</sharedItems>");
            } else {
                s.push_str("/>");
            }
        }
        if let Some(g) = &f.group {
            s.push_str("<fieldGroup");
            if let Some(p) = g.par {
                let _ = write!(s, " par=\"{p}\"");
            }
            let _ = write!(s, " base=\"{}\">", g.base);
            match &g.number {
                // [MS-XLSX] CT_RangePr for a number field: the bounds and the interval with
                // `groupBy="range"`; each group's caption is its item's `<s>` text.
                Some(ng) => {
                    // `groupBy` defaults to "range"; Excel omits it, and an explicit value with a
                    // start that does not cover the data made it refuse the file.
                    let _ = write!(
                        s,
                        "<rangePr startNum=\"{}\" endNum=\"{}\" groupInterval=\"{}\"",
                        crate::xml::num(ng.start),
                        crate::xml::num(ng.end),
                        crate::xml::num(ng.interval)
                    );
                }
                None => {
                    let _ = write!(s, "<rangePr groupBy=\"{}\"", group_by_name(g.by));
                    if let (Some(a), Some(b)) = (iso(sys, g.start), iso(sys, g.end)) {
                        let _ = write!(s, " startDate=\"{a}\" endDate=\"{b}\"");
                    }
                }
            }
            let _ = write!(s, "/><groupItems count=\"{}\">", g.items.len());
            for it in &g.items {
                let _ = write!(s, "<s v=\"{}\"/>", esc_attr(it));
            }
            s.push_str("</groupItems></fieldGroup>");
        }
        s.push_str("</cacheField>");
    }
    s.push_str("</cacheFields>");
    // Calculated items live on the cache: the formula in Excel's `Field[item]` form, the item's
    // index among the field's shared items, and the field's position on the axis.
    let mut calc: Vec<(usize, usize, usize, String)> = Vec::new();
    for (fi, f) in c.fields.iter().enumerate() {
        if f.calc_items.is_empty() {
            continue;
        }
        let Some(pos) = c.axis_map.iter().position(|x| *x == Some(fi)) else { continue };
        for (k, item) in f.calc_items.iter().enumerate() {
            let Some(compiled) = pivot_item::compile(&f.name, &item.formula).ok() else { continue };
            calc.push((fi, f.items.len() + k, pos, compiled.stored_form(&f.name)));
        }
    }
    if !calc.is_empty() {
        let _ = write!(s, "<calculatedItems count=\"{}\">", calc.len());
        for (field, item, pos, formula) in &calc {
            let _ = write!(
                s,
                "<calculatedItem formula=\"{}\"><pivotArea cacheIndex=\"{cache_id}\" outline=\"0\" fieldPosition=\"{pos}\"><references count=\"1\"><reference field=\"{field}\" count=\"1\"><x v=\"{item}\"/></reference></references></pivotArea></calculatedItem>",
                esc_attr(formula)
            );
        }
        s.push_str("</calculatedItems>");
    }
    if !ext.is_empty() {
        let _ = write!(s, "<extLst>{ext}</extLst>");
    }
    s.push_str("</pivotCacheDefinition>");
    s
}
fn table_xml(pt: &PivotTable, idx: usize, cache_id: u32, c: &Cache, fmt_id: &dyn Fn(&str) -> Option<u32>, ext: &str) -> String {
    let nf = c.fields.len();
    let find_db = |name: &str| c.fields.iter().position(|f| f.col.is_some() && same(&f.name, name));
    let row_idx: Vec<usize> = c.axis_map.iter().take(pt.rows.len()).filter_map(|x| *x).collect();
    let col_idx: Vec<usize> = c.axis_map.iter().skip(pt.rows.len()).filter_map(|x| *x).collect();
    // Filters: on an axis field → hidden items there; otherwise a page field.
    let mut hidden: HashMap<usize, &PivotFilter> = HashMap::new();
    let mut pages: Vec<(usize, &PivotFilter)> = Vec::new();
    for f in &pt.filters {
        let axis_pos = pt.rows.iter().chain(pt.columns.iter()).position(|a| same(&a.source_col, &f.source_col));
        if f.is_page(axis_pos.is_some()) {
            if let Some(i) = find_db(&f.source_col)
                && !pages.iter().any(|(j, _)| *j == i)
            {
                pages.push((i, f));
                hidden.insert(i, f);
            }
        } else if let Some(Some(i)) = axis_pos.and_then(|p| c.axis_map.get(p)) {
            hidden.insert(*i, f);
        }
    }
    let find_calc = |name: &str| c.fields.iter().position(|f| f.formula.is_some() && same(&f.name, name));
    let vals: Vec<(usize, &PivotValue)> =
        pt.values.iter().filter_map(|v| find_db(&v.source_col).or_else(|| find_calc(&v.source_col)).map(|i| (i, v))).collect();
    let axis_field = |i: usize| -> Option<&PivotField> {
        let p = c.axis_map.iter().position(|x| *x == Some(i))?;
        pt.rows.iter().chain(pt.columns.iter()).nth(p)
    };

    let compact = pt.layout == PivotLayout::Compact;
    let name = if pt.name.trim().is_empty() { format!("PivotTable{}", idx + 1) } else { pt.name.clone() };
    let mut s = format!(
        "{XML_DECL}<pivotTableDefinition xmlns=\"{NS_MAIN}\" name=\"{}\" cacheId=\"{cache_id}\" applyNumberFormats=\"0\" applyBorderFormats=\"0\" applyFontFormats=\"0\" applyPatternFormats=\"0\" applyAlignmentFormats=\"0\" applyWidthHeightFormats=\"1\" dataCaption=\"Values\" updatedVersion=\"6\" minRefreshableVersion=\"3\" useAutoFormatting=\"1\" itemPrintTitles=\"1\" createdVersion=\"6\" indent=\"0\"",
        esc_attr(&name)
    );
    match pt.layout {
        PivotLayout::Compact => s.push_str(" outline=\"1\" outlineData=\"1\""),
        PivotLayout::Outline => s.push_str(" compact=\"0\" compactData=\"0\" outline=\"1\" outlineData=\"1\""),
        PivotLayout::Tabular => s.push_str(" compact=\"0\" compactData=\"0\""),
    }
    if pt.values_on_rows {
        s.push_str(" dataOnRows=\"1\"");
    }
    if !pt.show_headers {
        s.push_str(" showHeaders=\"0\"");
    }
    // `rowGrandTotals` is the grand total row below the report (Excel's "On for Columns"),
    // `colGrandTotals` the grand total column at its right ("On for Rows").
    if !pt.grand_totals_cols {
        s.push_str(" rowGrandTotals=\"0\"");
    }
    if !pt.grand_totals_rows {
        s.push_str(" colGrandTotals=\"0\"");
    }
    s.push_str(" multipleFieldFilters=\"0\">");

    // Location: the body (report filters sit above it).
    let a = pt.anchor;
    let end = pt.last_range.map(|r| r.end).unwrap_or(a);
    let loc = RangeRef::new(a, CellRef::new(end.row.max(a.row), end.col.max(a.col)));
    let k = vals.len();
    // Σ Values (two or more value fields): its index among the written fields of its axis.
    let (row_map, _) = c.axis_map.split_at(pt.rows.len().min(c.axis_map.len()));
    let values_at = |map: &[Option<usize>], n: usize| pt.values_position.map_or(n, |p| map.iter().take(p).filter(|x| x.is_some()).count());
    let row_values = (k > 1 && pt.values_on_rows).then(|| values_at(row_map, row_idx.len()));
    // In Columns Σ Values comes after the last field: the column layout doesn't place it elsewhere.
    let col_values = (k > 1 && !pt.values_on_rows).then_some(col_idx.len());
    let first_data_row = if col_idx.is_empty() { 1 } else { 1 + col_idx.len() + usize::from(col_values.is_some()) };
    let row_levels = row_idx.len() + usize::from(row_values.is_some());
    let first_data_col = if row_levels == 0 && col_idx.is_empty() {
        0
    } else if compact {
        1
    } else {
        row_levels.max(1)
    };
    // Two or more value fields in Columns alone: Excel 365 would add a `Values` caption row.
    let hide_values_row = col_values.is_some() && col_idx.is_empty();
    let first_header_row = usize::from(!hide_values_row);
    let _ = write!(
        s,
        "<location ref=\"{}\" firstHeaderRow=\"{first_header_row}\" firstDataRow=\"{first_data_row}\" firstDataCol=\"{first_data_col}\"",
        loc.a1()
    );
    if !pages.is_empty() {
        let _ = write!(s, " rowPageCount=\"{}\" colPageCount=\"1\"", pages.len());
    }
    s.push_str("/>");

    // Fields.
    let _ = write!(s, "<pivotFields count=\"{nf}\">");
    for (i, f) in c.fields.iter().enumerate() {
        s.push_str("<pivotField");
        let axis = if row_idx.contains(&i) {
            Some("axisRow")
        } else if col_idx.contains(&i) {
            Some("axisCol")
        } else if pages.iter().any(|(j, _)| *j == i) {
            Some("axisPage")
        } else {
            None
        };
        if let Some(ax) = axis {
            let _ = write!(s, " axis=\"{ax}\"");
        }
        if vals.iter().any(|(j, _)| *j == i) {
            s.push_str(" dataField=\"1\"");
        }
        if f.formula.is_some() {
            s.push_str(" dragToRow=\"0\" dragToCol=\"0\" dragToPage=\"0\"");
        }
        match pt.layout {
            PivotLayout::Compact => {}
            PivotLayout::Outline => s.push_str(" compact=\"0\""),
            PivotLayout::Tabular => s.push_str(" compact=\"0\" outline=\"0\""),
        }
        if f.group.as_ref().is_some_and(|g| g.by != PivotDateGroup::None) {
            s.push_str(" numFmtId=\"14\"");
        }
        let filter = hidden.get(&i).copied();
        let labels = f.item_labels();
        let is_page = axis == Some("axisPage");
        // Hidden items; never hide every item (a field needs one visible item).
        let mut hide: Vec<bool> = match filter.and_then(|f| f.selected.as_ref()) {
            Some(sel) if !(is_page && sel.len() == 1) => labels.iter().map(|l| !sel.iter().any(|x| same(x, l))).collect(),
            _ => vec![false; labels.len()],
        };
        // A calculated item is never hidden by an item selection: its caption is not one of the
        // field's source items, and the report always shows it (record-level filters only).
        let calc_from = labels.len().saturating_sub(f.calc_items.len());
        for h in hide.iter_mut().skip(calc_from) {
            *h = false;
        }
        if !hide.is_empty() && hide.iter().all(|h| *h) {
            hide.iter_mut().for_each(|h| *h = false);
        }
        if is_page && hide.iter().any(|h| *h) {
            s.push_str(" multipleItemSelectionAllowed=\"1\"");
        }
        s.push_str(" showAll=\"0\"");
        if let Some(af) = axis_field(i) {
            match af.sort {
                PivotSort::Asc => s.push_str(" sortType=\"ascending\""),
                PivotSort::Desc => s.push_str(" sortType=\"descending\""),
                PivotSort::None => {}
            }
        }
        if !pt.subtotals {
            s.push_str(" defaultSubtotal=\"0\"");
        }
        if !pt.subtotals_top {
            s.push_str(" subtotalTop=\"0\"");
        }
        if axis.is_some() {
            let collapsed: &[String] = axis_field(i).map(|f| f.collapsed_items.as_slice()).unwrap_or(&[]);
            let count = labels.len() + usize::from(pt.subtotals);
            let _ = write!(s, "><items count=\"{count}\">");
            // The calculated items are the tail of `labels` (their captions were appended last).
            for (x, l) in labels.iter().enumerate() {
                let _ = write!(s, "<item x=\"{x}\"");
                if x >= calc_from && !f.calc_items.is_empty() {
                    s.push_str(" f=\"1\"");
                }
                if hide.get(x).copied().unwrap_or(false) {
                    s.push_str(" h=\"1\"");
                }
                if collapsed.iter().any(|cl| same(cl, l)) {
                    s.push_str(" sd=\"0\"");
                }
                s.push_str("/>");
            }
            if pt.subtotals {
                s.push_str("<item t=\"default\"/>");
            }
            s.push_str("</items></pivotField>");
        } else {
            s.push_str("/>");
        }
    }
    s.push_str("</pivotFields>");

    axis_fields_xml(&mut s, "rowFields", &row_idx, row_values);
    axis_fields_xml(&mut s, "colFields", &col_idx, col_values);
    if !pages.is_empty() {
        let _ = write!(s, "<pageFields count=\"{}\">", pages.len());
        for (i, f) in &pages {
            let _ = write!(s, "<pageField fld=\"{i}\"");
            if let Some(sel) = &f.selected
                && sel.len() == 1
                && let Some(one) = sel.first()
                && let Some(x) = c.fields.get(*i).and_then(|cf| cf.item_labels().iter().position(|l| same(l, one)))
            {
                let _ = write!(s, " item=\"{x}\"");
            }
            s.push_str(" hier=\"-1\"/>");
        }
        s.push_str("</pageFields>");
    }
    if !vals.is_empty() {
        let _ = write!(s, "<dataFields count=\"{k}\">");
        let mut used: Vec<String> = Vec::new();
        for (i, v) in &vals {
            let base = if v.name.trim().is_empty() {
                format!("{} of {}", func_caption(v.func), c.fields.get(*i).map(|f| f.name.as_str()).unwrap_or(""))
            } else {
                v.name.clone()
            };
            let mut n = base.clone();
            let mut j = 2;
            while used.iter().any(|u| same(u, &n)) {
                n = format!("{base}{j}");
                j += 1;
            }
            used.push(n.clone());
            let _ = write!(s, "<dataField name=\"{}\" fld=\"{i}\"", esc_attr(&n));
            if v.func != PivotFunc::Sum {
                let _ = write!(s, " subtotal=\"{}\"", func_name(v.func));
            }
            let base_field = row_idx.first().or(col_idx.first()).copied().unwrap_or(0);
            match v.show_as {
                PivotShowAs::Normal => s.push_str(" baseField=\"0\" baseItem=\"0\""),
                PivotShowAs::PercentOfGrandTotal => s.push_str(" showDataAs=\"percentOfTotal\" baseField=\"0\" baseItem=\"0\""),
                PivotShowAs::PercentOfColumnTotal => s.push_str(" showDataAs=\"percentOfCol\" baseField=\"0\" baseItem=\"0\""),
                PivotShowAs::PercentOfRowTotal => s.push_str(" showDataAs=\"percentOfRow\" baseField=\"0\" baseItem=\"0\""),
                PivotShowAs::RunningTotal => {
                    let _ = write!(s, " showDataAs=\"runTotal\" baseField=\"{base_field}\" baseItem=\"0\"");
                }
                PivotShowAs::Rank => {
                    let _ = write!(s, " baseField=\"{base_field}\" baseItem=\"0\"");
                }
            }
            if let Some(id) = v.number_format.as_deref().and_then(fmt_id) {
                let _ = write!(s, " numFmtId=\"{id}\"");
            }
            if v.show_as == PivotShowAs::Rank {
                let _ = write!(
                    s,
                    "><extLst><ext uri=\"{EXT_X14_DATA_FIELD}\" xmlns:x14=\"{NS_X14}\"><x14:dataField pivotShowAs=\"rankDescending\"/></ext></extLst></dataField>"
                );
            } else {
                s.push_str("/>");
            }
        }
        s.push_str("</dataFields>");
    }
    s.push_str("<pivotTableStyleInfo");
    if !pt.style.is_empty() {
        let _ = write!(s, " name=\"{}\"", esc_attr(&pt.style));
    }
    s.push_str(" showRowHeaders=\"1\" showColHeaders=\"1\" showRowStripes=\"0\" showColStripes=\"0\" showLastColumn=\"1\"/>");
    // One `<extLst>`: the extension Cella writes for the model, then the source's own.
    let mut exts = String::new();
    if hide_values_row {
        let _ = write!(exts, "<ext uri=\"{EXT_X14_PIVOT_TABLE}\" xmlns:x14=\"{NS_X14}\"><x14:pivotTableDefinition hideValuesRow=\"1\"/></ext>");
    }
    exts.push_str(ext);
    if !exts.is_empty() {
        let _ = write!(s, "<extLst>{exts}</extLst>");
    }
    s.push_str("</pivotTableDefinition>");
    s
}
/// `<rowFields>`/`<colFields>`: the axis's cache field indices, with Σ Values (`-2`) at `values_at`.
fn axis_fields_xml(s: &mut String, tag: &str, idx: &[usize], values_at: Option<usize>) {
    let n = idx.len() + usize::from(values_at.is_some());
    if n == 0 {
        return;
    }
    let _ = write!(s, "<{tag} count=\"{n}\">");
    for (j, i) in idx.iter().enumerate() {
        if values_at == Some(j) {
            s.push_str("<field x=\"-2\"/>");
        }
        let _ = write!(s, "<field x=\"{i}\"/>");
    }
    if values_at.is_some_and(|p| p >= idx.len()) {
        s.push_str("<field x=\"-2\"/>");
    }
    let _ = write!(s, "</{tag}>");
}
