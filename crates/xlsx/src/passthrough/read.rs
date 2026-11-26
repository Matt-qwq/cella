//! Reading the parts Cella keeps verbatim, with their content type and relationships.

use super::rebase::*;
use super::*;

/// The model's PivotTable for the pivotTable part `part`, when Cella reads it but does not
/// compute it (a data-model or OLAP cache). Its parts are carried whole instead of rewritten.
fn read_only_pivot<'a>(cx: &mut Ctx<'_>, part: &str, pivots: &'a [PivotTable]) -> Option<&'a PivotTable> {
    if !pivots.iter().any(PivotTable::is_read_only) {
        return None;
    }
    let bytes = cx.pkg.read_limited(part, MAX_META).ok().flatten()?;
    let name = xml::parse(&bytes).ok()?.attr("name").map(str::to_string)?;
    pivots.iter().find(|p| p.is_read_only() && p.name.eq_ignore_ascii_case(name.trim()))
}
/// A part's relationships with full type URIs. `Ok(vec![])` when it has none; an error when the
/// rels part is damaged or too large (its part then can't be kept faithfully).
fn raw_rels(cx: &mut Ctx<'_>, part: &str) -> Result<Vec<PreservedRel>, String> {
    let name = rels_part_name(part);
    let bytes = match cx.pkg.read_limited(&name, MAX_META) {
        Ok(Some(b)) => b,
        Ok(None) => return Ok(vec![]),
        Err(e) => return Err(e.to_string()),
    };
    let root = xml::parse(&bytes).map_err(|e| format!("{name}: {e}"))?;
    let (dir, _) = package::split_dir(part);
    let mut out = vec![];
    for r in root.kids("Relationship") {
        if out.len() >= MAX_RELS {
            return Err(format!("{name}: too many relationships"));
        }
        let (Some(id), Some(target)) = (r.attr("Id"), r.attr("Target")) else { continue };
        let external = r.attr("TargetMode").is_some_and(|m| m.eq_ignore_ascii_case("External"));
        out.push(PreservedRel {
            id: id.to_string(),
            rel_type: r.attr("Type").unwrap_or("").to_string(),
            target: if external { target.to_string() } else { package::resolve(dir, target) },
            external,
        });
    }
    Ok(out)
}
// ---------------------------------------------------------------- reading

/// The root element's `name` attribute and the raw `<ext>` children of a part's `<extLst>`.
struct ExtScan {
    name: Option<String>,
    exts: Vec<String>,
}
/// Reads the `extLst` children of a part that Cella rewrites (a table, PivotTable or pivot
/// cache definition), with the root's namespace declarations added to each fragment. `None` when
/// the part is not XML.
fn scan_exts(bytes: &[u8]) -> Option<ExtScan> {
    let data = xml::to_utf8(bytes);
    let text = std::str::from_utf8(&data).ok()?;
    let mut reader = Reader::from_str(text);
    let mut decls: Vec<(String, String)> = vec![];
    let mut out = ExtScan { name: None, exts: vec![] };
    let mut in_root = false;
    let mut total = 0usize;
    loop {
        match reader.read_event().ok()? {
            Event::Eof => break,
            Event::Start(e) if !in_root => {
                in_root = true;
                for a in e.attributes().with_checks(false).flatten() {
                    if a.key.as_ref() == b"name" {
                        out.name = String::from_utf8(a.value.to_vec()).ok();
                    } else if let Some(p) = a.key.as_ref().strip_prefix(b"xmlns:")
                        && let (Ok(p), Ok(v)) = (std::str::from_utf8(p), std::str::from_utf8(&a.value))
                        && !v.contains('"')
                    {
                        decls.push((p.to_string(), v.to_string()));
                    }
                }
            }
            Event::Start(e) => {
                let local = e.local_name().as_ref().to_vec();
                if local == b"extLst" {
                    read_ext_list(&mut reader, text, &decls, &mut out.exts, &mut total)?;
                } else {
                    reader.read_to_end(e.name()).ok()?;
                }
            }
            Event::End(_) => break,
            _ => {}
        }
    }
    Some(out)
}
/// Reads the `<ext>` children of the `<extLst>` the reader is positioned in, appending their raw
/// XML to `out`. Bounded: at most [`MAX_EXTS`] fragments of [`MAX_FRAGMENT`] bytes.
fn read_ext_list(reader: &mut Reader<&[u8]>, text: &str, decls: &[(String, String)], out: &mut Vec<String>, total: &mut usize) -> Option<()> {
    loop {
        let start = usize::try_from(reader.buffer_position()).ok()?;
        match reader.read_event().ok()? {
            Event::Eof => return None,
            Event::Start(e) => {
                let is_ext = e.local_name().as_ref() == b"ext";
                reader.read_to_end(e.name()).ok()?;
                if is_ext {
                    let end = usize::try_from(reader.buffer_position()).ok()?;
                    push_ext(text, start, end, decls, out, total);
                }
            }
            Event::Empty(e) => {
                if e.local_name().as_ref() == b"ext" {
                    let end = usize::try_from(reader.buffer_position()).ok()?;
                    push_ext(text, start, end, decls, out, total);
                }
            }
            Event::End(_) => return Some(()),
            _ => {}
        }
    }
}
fn push_ext(text: &str, start: usize, end: usize, decls: &[(String, String)], out: &mut Vec<String>, total: &mut usize) {
    if out.len() >= MAX_EXTS || end.saturating_sub(start) > MAX_FRAGMENT {
        return;
    }
    let Some(frag) = text.get(start..end) else { return };
    let frag = with_decls(frag, decls);
    if total.saturating_add(frag.len()) > MAX_EXTS_BYTES {
        return;
    }
    *total = total.saturating_add(frag.len());
    out.push(frag);
}
/// Every relationship id a fragment names, in order; `None` when the fragment is damaged or a
/// namespace prefix it uses is undeclared.
fn rel_ids(x: &str) -> Option<Vec<String>> {
    let mut ids = vec![];
    map_rel_ids(x, &mut |id| {
        ids.push(id.to_string());
        Some(id.to_string())
    })
    .map(|_| ids)
}
/// A part Cella rewrites, as far as pass-through needs it: its root `name`, its `<extLst>`
/// children and its relationships.
fn read_owner_part(cx: &mut Ctx<'_>, part: &str) -> Option<(String, Vec<String>, Vec<PreservedRel>)> {
    let bytes = cx.pkg.read_limited(part, MAX_META).ok().flatten()?;
    let scan = scan_exts(&bytes)?;
    let rels = raw_rels(cx, part).ok()?;
    Some((scan.name.unwrap_or_default(), scan.exts, rels))
}
/// The `<sheet>` entries of the workbook that are chart sheets, each with the number of worksheets
/// (model sheets) before it.
fn chart_sheet_entries<'a>(wb_xml: &'a xml::El, wb_rels: &[PreservedRel], file_to_model: &[Option<usize>]) -> Vec<(usize, &'a xml::El)> {
    let mut out = vec![];
    let mut worksheets = 0usize;
    for (i, se) in wb_xml.child("sheets").into_iter().flat_map(|s| s.kids("sheet")).enumerate() {
        if file_to_model.get(i).copied().flatten().is_some() {
            worksheets += 1;
        } else if se.attr("id").and_then(|id| wb_rels.iter().find(|r| r.id == id)).is_some_and(|r| is_kind(r, &["chartsheet"])) {
            out.push((worksheets, se));
        }
    }
    out
}
/// Collects kept content while a package is read.
pub struct KeptReader {
    on: bool,
    strict: bool,
    defaults: HashMap<String, String>,
    overrides: HashMap<String, String>,
    budget: usize,
    parts_left: usize,
}
impl KeptReader {
    pub fn new(cx: &mut Ctx<'_>) -> KeptReader {
        let mut k = KeptReader {
            on: true,
            strict: false,
            defaults: HashMap::new(),
            overrides: HashMap::new(),
            budget: kept_total(),
            parts_left: MAX_KEPT_PARTS,
        };
        if let Ok(root) = raw_rels(cx, "")
            && root.iter().any(|r| r.rel_type.starts_with("http://purl.oclc.org/"))
        {
            k.on = false;
            k.strict = true;
        }
        match cx.pkg.read_limited("[Content_Types].xml", MAX_META).ok().flatten().and_then(|b| xml::parse(&b).ok()) {
            Some(types) => {
                for d in types.kids("Default") {
                    if let (Some(e), Some(t)) = (d.attr("Extension"), d.attr("ContentType")) {
                        k.defaults.insert(e.to_ascii_lowercase(), t.to_string());
                    }
                }
                for o in types.kids("Override") {
                    if let (Some(p), Some(t)) = (o.attr("PartName"), o.attr("ContentType")) {
                        k.overrides.insert(normalize(p), t.to_string());
                    }
                }
            }
            None => k.on = false,
        }
        k
    }

    fn content_type(&self, part: &str) -> Option<String> {
        let key = normalize(part);
        if let Some(t) = self.overrides.get(&key) {
            return Some(t.clone());
        }
        let (_, file) = package::split_dir(&key);
        let ext = file.rsplit_once('.').map(|(_, e)| e)?;
        self.defaults.get(ext).cloned()
    }

    /// The parts reachable from `root` that aren't in `seen` yet. Fails (and keeps nothing) when the
    /// graph reaches rewritten content, a missing or damaged part, or a size limit.
    fn closure(&mut self, cx: &mut Ctx<'_>, root: &PreservedRel, seen: &HashSet<String>, through: &[&str]) -> Result<Vec<PreservedPart>, String> {
        if root.external {
            return Ok(vec![]);
        }
        let blocked = |r: &PreservedRel| rewritten(r) && !through.iter().any(|k| k.eq_ignore_ascii_case(kind(&r.rel_type)));
        if blocked(root) {
            return Err(format!("it refers to {}", kind(&root.rel_type)));
        }
        let mut parts = vec![];
        let mut bytes = 0usize;
        let mut local: HashSet<String> = HashSet::new();
        let mut queue = VecDeque::from([(root.target.clone(), 0usize)]);
        while let Some((name, depth)) = queue.pop_front() {
            let key = normalize(&name);
            if seen.contains(&key) || !local.insert(key) {
                continue;
            }
            if depth > MAX_CHAIN {
                return Err("its relationships are nested too deeply".into());
            }
            if parts.len() >= self.parts_left {
                return Err("it has too many parts".into());
            }
            let content_type = self.content_type(&name).ok_or_else(|| format!("{name} has no content type"))?;
            let data = match cx.pkg.read_limited(&name, kept_part()) {
                Ok(Some(d)) => d,
                Ok(None) => return Err(format!("{name} is missing")),
                Err(e) => return Err(e.to_string()),
            };
            bytes = bytes.saturating_add(data.len());
            if bytes > self.budget {
                return Err("it is too large".into());
            }
            let rels = raw_rels(cx, &name)?;
            for r in rels.iter().filter(|r| !r.external) {
                if blocked(r) {
                    return Err(format!("it refers to {}", kind(&r.rel_type)));
                }
                queue.push_back((r.target.clone(), depth + 1));
            }
            parts.push(PreservedPart { name, content_type, data: Arc::from(data), rels });
        }
        self.budget = self.budget.saturating_sub(bytes);
        self.parts_left = self.parts_left.saturating_sub(parts.len());
        Ok(parts)
    }

    /// Keeps a set of relationships (all or nothing) and the parts behind them.
    fn keep(&mut self, cx: &mut Ctx<'_>, p: &mut Preserved, roots: &[PreservedRel], seen: &mut HashSet<String>) -> Result<(), String> {
        self.keep_through(cx, p, roots, seen, &[])
    }

    /// [`Self::keep`] for roots whose graph may reach the rewritten kinds in `through`.
    fn keep_through(
        &mut self,
        cx: &mut Ctx<'_>,
        p: &mut Preserved,
        roots: &[PreservedRel],
        seen: &mut HashSet<String>,
        through: &[&str],
    ) -> Result<(), String> {
        let mut new_parts: Vec<PreservedPart> = vec![];
        let mut scratch = seen.clone();
        for r in roots {
            let parts = self.closure(cx, r, &scratch, through)?;
            scratch.extend(parts.iter().map(|q| normalize(&q.name)));
            new_parts.extend(parts);
        }
        *seen = scratch;
        p.parts.extend(new_parts);
        for r in roots {
            // The same part is often named twice: by an extension and by a root kind (a slicer
            // cache is related to the workbook and listed in its `<extLst>`). One relationship is
            // written, with the id the kept XML uses.
            if !p.rels.iter().any(|x| x.id == r.id || (x.rel_type == r.rel_type && x.target == r.target && x.external == r.external)) {
                p.rels.push(r.clone());
            }
        }
        Ok(())
    }

    /// Keeps a part Cella rewrites and what it owns that the model doesn't: the `<extLst>`
    /// children it has (except `modeled`, which Cella writes itself), the relationships in
    /// `owned` and everything behind them. Extras hang off the owner key (`owner_key`), so a
    /// renamed or deleted owner drops them instead of attaching them elsewhere. `key_name`
    /// overrides the part's root `name` attribute (a pivot cache definition has none).
    #[allow(clippy::too_many_arguments)]
    fn keep_owner(
        &mut self,
        cx: &mut Ctx<'_>,
        part: &str,
        kind_name: &str,
        key_name: Option<&str>,
        modeled: &[&str],
        owned: &[&str],
        p: &mut Preserved,
    ) -> Option<String> {
        let (name, exts, rels) = match read_owner_part(cx, part) {
            Some(x) => x,
            None => {
                cx.warn(format!("{part}: its unmodelled content could not be read and was dropped"));
                return None;
            }
        };
        let name = key_name.map(str::to_string).unwrap_or(name);
        let Some(key) = owner_key(kind_name, &name) else {
            cx.warn(format!("{part} has no usable name; its unmodelled content was dropped"));
            return None;
        };
        // A fresh set: a slicer cache can hang off two PivotTables (or the workbook too), and
        // each owner must get every part its own extensions name. The writer dedupes by part name.
        let mut seen = HashSet::new();
        self.keep_exts(cx, part, &rels, &exts, modeled, &key, owned, p, &mut seen);
        Some(key)
    }

    /// Keeps the `extLst` children of an owner that Cella does not model, and the
    /// relationships they name. `key` is empty for the package's own owners (the workbook and a
    /// worksheet, whose relationships are written as they are) and the owner key for a rewritten
    /// part. A fragment that names a relationship Cella can't keep is dropped whole: it would
    /// point at a part the file no longer has.
    #[allow(clippy::too_many_arguments)]
    fn keep_exts(
        &mut self,
        cx: &mut Ctx<'_>,
        owner: &str,
        rels: &[PreservedRel],
        frags: &[String],
        modeled: &[&str],
        key: &str,
        owned: &[&str],
        p: &mut Preserved,
        seen: &mut HashSet<String>,
    ) {
        let mut wanted: Vec<(&str, Vec<String>)> = vec![];
        for frag in frags {
            let Ok(el) = xml::parse(frag.as_bytes()) else {
                cx.warn(format!("{owner}: an extension was dropped: its XML is damaged"));
                continue;
            };
            let uri = el.attr("uri").unwrap_or("");
            let modeled_uri = modeled.iter().any(|m| m.eq_ignore_ascii_case(uri));
            let modeled_child = el.children.iter().any(|c| MODELED_SHEET_EXT_ELEMENTS.contains(&c.name.as_str()));
            if modeled_uri || modeled_child {
                continue;
            }
            let Some(ids) = rel_ids(frag) else {
                cx.warn(format!("{owner}: an extension was dropped: its XML is damaged"));
                continue;
            };
            let keepable = |id: &str| rels.iter().any(|r| r.id == id && !rewritten(r));
            if let Some(bad) = ids.iter().find(|id| !keepable(id)) {
                cx.warn(format!("{owner}: an extension was dropped: it names {bad}, which Cella cannot keep"));
                continue;
            }
            wanted.push((frag, ids));
        }
        let mut keep_ids: HashSet<String> = rels.iter().filter(|r| !rewritten(r) && is_kind(r, owned)).map(|r| r.id.clone()).collect();
        for (_, ids) in &wanted {
            keep_ids.extend(ids.iter().cloned());
        }
        // An extension that names no part is kept on its own: it may hold anything.
        if keep_ids.is_empty() && wanted.is_empty() {
            return;
        }
        let mut kept: HashSet<String> = HashSet::new();
        for r in rels.iter().filter(|r| keep_ids.contains(&r.id)) {
            let mut q = Preserved::default();
            match self.keep(cx, &mut q, std::slice::from_ref(r), seen) {
                Ok(()) => {
                    kept.insert(r.id.clone());
                    p.parts.extend(q.parts);
                    let id = if key.is_empty() { r.id.clone() } else { prefixed(key, &r.id) };
                    if !p.rels.iter().any(|x| x.id == id) {
                        p.rels.push(PreservedRel { id, ..r.clone() });
                    }
                }
                Err(why) => cx.warn(format!("{owner}: {} was dropped: {why}", kind(&r.rel_type))),
            }
        }
        for (frag, ids) in wanted {
            if ids.iter().all(|id| kept.contains(id)) {
                p.elements.push(PreservedXml { slot: if key.is_empty() { EXT_SLOT.into() } else { key.to_string() }, xml: frag.to_string() });
            } else {
                cx.warn(format!("{owner}: an extension was dropped: a part it names could not be kept"));
            }
        }
    }

    /// Package-root and workbook-level content.
    pub fn workbook(&mut self, cx: &mut Ctx<'_>, wb_part: &str, wb_xml: &xml::El, file_to_model: &[Option<usize>]) -> Option<Arc<Preserved>> {
        if !self.on {
            let wb_rels = raw_rels(cx, wb_part).unwrap_or_default();
            for (_, se) in chart_sheet_entries(wb_xml, &wb_rels, file_to_model) {
                cx.warn(format!("chart sheet \"{}\" was skipped and is not kept on save", se.attr("name").unwrap_or("")));
            }
            if self.strict {
                cx.warn("unmodelled content of a Strict Open XML workbook (custom XML, links, controls…) is not kept on save");
            }
            return None;
        }
        let mut p = Preserved::default();
        let mut seen = HashSet::new();
        // Package root: custom document properties, ribbon customisation, task panes.
        if let Ok(root) = raw_rels(cx, "") {
            for r in root.iter().filter(|r| is_kind(r, PACKAGE_ROOTS)) {
                let mut q = Preserved::default();
                match self.keep(cx, &mut q, std::slice::from_ref(r), &mut seen) {
                    Ok(()) => {
                        p.parts.extend(q.parts);
                        p.package_rels.extend(q.rels);
                    }
                    Err(why) => cx.warn(format!("{} was dropped: {why}", kind(&r.rel_type))),
                }
            }
        }
        let wb_rels = raw_rels(cx, wb_part).unwrap_or_default();
        for r in wb_rels.iter().filter(|r| is_kind(r, WORKBOOK_ROOTS)) {
            if let Err(why) = self.keep(cx, &mut p, std::slice::from_ref(r), &mut seen) {
                cx.warn(format!("{} part {} was dropped: {why}", kind(&r.rel_type), r.target));
            }
        }
        // Workbook extensions Cella does not model (slicer caches, timeline cache references,
        // defined names of extensions…), and the slicer and timeline caches they or a relationship
        // name. Their part bytes, relationships and content types are kept as they are.
        let wb_exts = cx.pkg.read_limited(wb_part, MAX_META).ok().flatten().and_then(|b| scan_exts(&b)).map(|s| s.exts).unwrap_or_default();
        self.keep_exts(cx, wb_part, &wb_rels, &wb_exts, &[CALC_FEATURES_URI], "", WORKBOOK_CACHES, &mut p, &mut seen);
        // The data model (Power Pivot): the parts hold the user's own model — the tables, measures
        // and relationships of Power Pivot — and are worth more than the reports that point at
        // them, so they hang off the workbook instead of a PivotTable. They come back byte for
        // byte even after every report built on them is deleted.
        self.keep_data_model(cx, wb_xml, &wb_rels, &mut p, &mut seen);
        // Chart sheets: the sheet entry (name, state, position) plus the chart sheet part and the
        // drawing, chart, style and colour parts behind it.
        for (worksheets, se) in chart_sheet_entries(wb_xml, &wb_rels, file_to_model) {
            let name = se.attr("name").unwrap_or("").to_string();
            let Some(r) = se.attr("id").and_then(|id| wb_rels.iter().find(|r| r.id == id)) else { continue };
            match self.keep_through(cx, &mut p, std::slice::from_ref(r), &mut seen, CHART_SHEET_GRAPH) {
                Ok(()) => p.sheets.push(PreservedSheet {
                    name,
                    visibility: match se.attr("state") {
                        Some("hidden") => Visibility::Hidden,
                        Some("veryHidden") => Visibility::VeryHidden,
                        _ => Visibility::Visible,
                    },
                    rel_id: r.id.clone(),
                    before: worksheets,
                }),
                Err(why) => cx.warn(format!("chart sheet \"{name}\" was dropped: {why}")),
            }
        }
        // Links to external workbooks: `[1]` in formulas is the position in <externalReferences>,
        // so they are kept all together or not at all.
        let ids: Vec<&str> =
            wb_xml.child("externalReferences").map(|e| e.kids("externalReference").filter_map(|x| x.attr("id")).collect()).unwrap_or_default();
        if !ids.is_empty() {
            let roots: Option<Vec<PreservedRel>> =
                ids.iter().map(|id| wb_rels.iter().find(|r| r.id == *id && is_kind(r, &["externalLink"])).cloned()).collect();
            let kept = match roots {
                Some(roots) => self.keep(cx, &mut p, &roots, &mut seen),
                None => Err("a link has no part".into()),
            };
            match kept {
                Ok(()) => {
                    let mut x = format!("<externalReferences xmlns:r=\"{NS_REL}\">");
                    for id in &ids {
                        let _ = write!(x, "<externalReference r:id=\"{}\"/>", esc_attr(id));
                    }
                    x.push_str("</externalReferences>");
                    p.elements.push(PreservedXml { slot: "externalReferences".into(), xml: x });
                    cx.warn("links to external workbooks were kept; they are not followed or refreshed");
                }
                Err(why) => cx.warn(format!("links to external workbooks were dropped: {why}")),
            }
        }
        // Excel trusts the saved cell values only when both are present; without them it
        // recalculates everything on open, and links into closed workbooks come back as errors.
        if let Some(v) = wb_xml.child("fileVersion") {
            let mut x = String::from("<fileVersion");
            for k in FILE_VERSION_ATTRS {
                if let Some(a) = v.attr(k) {
                    let _ = write!(x, " {k}=\"{}\"", esc_attr(a));
                }
            }
            x.push_str("/>");
            p.elements.push(PreservedXml { slot: "fileVersion".into(), xml: x });
        }
        let calc_features = wb_xml
            .child("extLst")
            .and_then(|l| l.kids("ext").find(|e| e.attr("uri").is_some_and(|u| u.eq_ignore_ascii_case(CALC_FEATURES_URI))))
            .and_then(|e| e.child("calcFeatures"));
        if let Some(c) = calc_features {
            let mut x = format!("<ext uri=\"{CALC_FEATURES_URI}\" xmlns:xcalcf=\"{NS_CALC_FEATURES}\"><xcalcf:calcFeatures>");
            for name in c.kids("feature").filter_map(|f| f.attr("name")) {
                let _ = write!(x, "<xcalcf:feature name=\"{}\"/>", esc_attr(name));
            }
            x.push_str("</xcalcf:calcFeatures></ext>");
            p.elements.push(PreservedXml { slot: CALC_SLOT.into(), xml: x });
        }
        (!p.is_empty()).then(|| Arc::new(p))
    }

    /// The parts of the workbook's data model (Power Pivot), kept with the workbook however the
    /// reports that name them fare. Two shapes name it: a cache definition relating a
    /// `powerPivotData` part (the shape the older fixture here has), and the one Excel writes —
    /// the *workbook* relating `model/item.data`. Either way its content is opaque to Cella and is
    /// never read.
    fn keep_data_model(&mut self, cx: &mut Ctx<'_>, wb_xml: &xml::El, wb_rels: &[PreservedRel], p: &mut Preserved, seen: &mut HashSet<String>) {
        let caches: Vec<String> = wb_xml
            .child("pivotCaches")
            .map(|l| l.kids("pivotCache").filter_map(|c| c.attr("id")).map(str::to_string).collect::<Vec<_>>())
            .unwrap_or_default();
        // Excel relates the model from the **workbook** (`xl/_rels/workbook.xml.rels`,
        // `powerPivotData` -> `model/item.data`); a file that hangs it off a cache definition
        // instead is kept too. Measured on a workbook Excel saved with "Add this data to the Data
        // Model": only the workbook had the relationship, so looking at the caches alone lost it.
        let mut roots: Vec<PreservedRel> = wb_rels.iter().filter(|r| !r.external && is_kind(r, POWER_PIVOT_KINDS)).cloned().collect();
        for id in caches {
            let Some(cache) = wb_rels.iter().find(|r| r.id == id && !r.external && is_kind(r, &["pivotCacheDefinition"])) else { continue };
            roots.extend(raw_rels(cx, &cache.target).unwrap_or_default().into_iter().filter(|r| !r.external && is_kind(r, POWER_PIVOT_KINDS)));
        }
        if roots.is_empty() {
            return;
        }
        // One root per part: a model shared by two caches is written once (the writer dedupes).
        let mut seen_parts: HashSet<String> = HashSet::new();
        roots.retain(|r| seen_parts.insert(normalize(&r.target)));
        match self.keep_through(cx, p, &roots, seen, PIVOT_GRAPH) {
            Ok(()) => cx.warn("the workbook's data model was kept; Cella carries it and does not read it"),
            Err(why) => cx.warn(format!("the workbook's data model was dropped: {why}")),
        }
    }

    /// Sheet-level content of the sheet part `part`. `pivots` are the PivotTables the model read
    /// from it: a report Cella does not compute is carried whole, not rewritten.
    pub fn sheet(&mut self, cx: &mut Ctx<'_>, part: &str, name: &str, pivots: &[PivotTable]) -> Option<Arc<Preserved>> {
        if !self.on {
            return None;
        }
        let rels = match raw_rels(cx, part) {
            Ok(r) => r,
            Err(e) => {
                cx.warn(format!("sheet \"{name}\": relationships could not be read ({e}); unmodelled content was dropped"));
                return None;
            }
        };
        let mut p = Preserved::default();
        let mut seen = HashSet::new();
        for r in rels.iter().filter(|r| is_kind(r, SHEET_ROOTS)) {
            if let Err(why) = self.keep(cx, &mut p, std::slice::from_ref(r), &mut seen) {
                cx.warn(format!("sheet \"{name}\": {} was dropped: {why}", kind(&r.rel_type)));
            }
        }
        // Parts Cella rewrites keep what the model has no room for: a table's extensions and
        // the query table attached to it, a PivotTable's slicer cache and the extensions of its
        // cache definition. Each hangs off the owner's name; a renamed or deleted owner drops it.
        for r in rels.iter().filter(|r| is_kind(r, &["table"]) && !r.external) {
            self.keep_owner(cx, &r.target, "table", None, &[], TABLE_OWNED, &mut p);
        }
        for r in rels.iter().filter(|r| is_kind(r, &["pivotTable"]) && !r.external) {
            // A report Cella does not compute (a data-model or OLAP cache) is carried whole:
            // its own part, its cache definition, its records and the data model behind them are
            // kept byte for byte, keyed to the report's name so a deleted or renamed report takes
            // them with it. Its cached values stay on the sheet as the cells they already are.
            //
            // The pivot's relationships that name another part (the cache definition, then its
            // records) are followed through the kinds Cella normally rewrites: keeping the
            // whole graph is the point, and the graph is never recomputed.
            if let Some(read_only) = read_only_pivot(cx, &r.target, pivots) {
                let root = owner_key("pivot", &read_only.name).map(|key| PreservedRel { id: prefixed(&key, &r.id), ..r.clone() });
                match root.map(|root| self.keep_through(cx, &mut p, std::slice::from_ref(&root), &mut seen, PIVOT_GRAPH)) {
                    Some(Ok(())) => continue,
                    Some(Err(why)) => cx.warn(format!("the read-only PivotTable \"{}\" was dropped: {why}", read_only.name)),
                    None => cx.warn(format!("PivotTable \"{}\" has no usable name; its parts were dropped", read_only.name)),
                }
            }
            let key = self.keep_owner(cx, &r.target, "pivot", None, MODELED_PIVOT_EXTS, &[], &mut p);
            if let Some(key) = key
                && let Some(cache) =
                    raw_rels(cx, &r.target).unwrap_or_default().into_iter().find(|q| is_kind(q, &["pivotCacheDefinition"]) && !q.external)
            {
                let pivot_name = key.split_once(':').map(|(_, n)| n).unwrap_or("");
                self.keep_owner(cx, &cache.target, "pivotcache", Some(pivot_name), &[], &[], &mut p);
            }
        }
        // Notes have one VML drawing per sheet; another one holds the header and footer pictures.
        let vml_drawings = rels.iter().filter(|r| is_kind(r, &["vmlDrawing"])).count();
        let notes = rels.iter().filter(|r| is_kind(r, &["comments"])).count();
        if rels.iter().any(|r| is_kind(r, SHEET_ELEMENT_KINDS)) || vml_drawings > notes || cx.sheet_ext {
            self.sheet_elements(cx, part, name, &rels, &mut p, &mut seen);
        }
        (!p.is_empty()).then(|| Arc::new(p))
    }

    /// A chart sheet's own content that the model has no room for: page margins, page setup, header
    /// and footer, protection and its extensions. Each child is kept as a fragment, its relationship
    /// ids rewritten on save (printer settings, the only part a chart sheet may reference that
    /// Cella does not write, are kept with it). The drawing and the chart behind it are
    /// modelled, so they are not kept here: writing them twice would give the package two charts.
    pub fn chart_sheet(&mut self, cx: &mut Ctx<'_>, part: &str, name: &str) -> Option<Arc<Preserved>> {
        if !self.on {
            return None;
        }
        let bytes = match cx.pkg.read_limited(part, MAX_META) {
            Ok(Some(b)) => b,
            Ok(None) => return None,
            Err(IoError::TooLarge(_)) => {
                cx.warn(format!("chart sheet \"{name}\": its page setup and header could not be read (the part is too large) and were dropped"));
                return None;
            }
            Err(e) => {
                cx.warn(format!("chart sheet \"{name}\": its page setup and header could not be read ({e}) and were dropped"));
                return None;
            }
        };
        let children = match scan_chart_sheet(&bytes) {
            Some(c) => c,
            None => {
                cx.warn(format!("chart sheet \"{name}\": its page setup and header could not be read and were dropped"));
                return None;
            }
        };
        let rels = match raw_rels(cx, part) {
            Ok(r) => r,
            Err(e) => {
                cx.warn(format!("chart sheet \"{name}\": its relationships could not be read ({e}); unmodelled content was dropped"));
                return None;
            }
        };
        let mut p = Preserved::default();
        let mut seen = HashSet::new();
        for (tag, xml, ids) in children {
            // The drawing (and the chart it frames) is written from the model, and so is <sheetPr>
            // (tab colour and fit-to-page are model fields).
            if tag == "drawing" || tag == "sheetPr" {
                continue;
            }
            let roots: Option<Vec<PreservedRel>> =
                ids.iter().map(|id| rels.iter().find(|r| r.id == *id && !r.external && is_kind(r, &["printerSettings"])).cloned()).collect();
            let Some(roots) = roots else {
                cx.warn(format!("chart sheet \"{name}\": its <{tag}> names a part Cella does not keep, so it was dropped"));
                continue;
            };
            if !roots.is_empty()
                && let Err(why) = self.keep(cx, &mut p, &roots, &mut seen)
            {
                cx.warn(format!("chart sheet \"{name}\": its <{tag}> was dropped: {why}"));
                continue;
            }
            p.elements.push(PreservedXml { slot: format!("{CHART_SHEET_SLOT}{tag}"), xml });
        }
        (!p.is_empty()).then(|| Arc::new(p))
    }

    fn sheet_elements(&mut self, cx: &mut Ctx<'_>, part: &str, name: &str, rels: &[PreservedRel], p: &mut Preserved, seen: &mut HashSet<String>) {
        let scan = match cx.pkg.read(part) {
            Ok(Some(bytes)) => scan_sheet(&bytes),
            _ => None,
        };
        let Some(scan) = scan else {
            cx.warn(format!("sheet \"{name}\": controls and embedded objects could not be read and were dropped"));
            return;
        };
        // The sheet's unmodelled extensions (timelines, filters, anything else): kept with the
        // parts their relationship ids name.
        self.keep_exts(cx, part, rels, &scan.exts, MODELED_SHEET_EXTS, "", &[], p, seen);
        // Printer settings (a binary part per sheet) hang off <pageSetup r:id>.
        if let Some(id) = scan.page_setup.as_deref()
            && let Some(r) = rels.iter().find(|r| r.id == id && is_kind(r, &["printerSettings"]))
        {
            match self.keep(cx, p, std::slice::from_ref(r), seen) {
                Ok(()) => p.elements.push(PreservedXml { slot: "pageSetup".into(), xml: r.id.clone() }),
                Err(why) => cx.warn(format!("sheet \"{name}\": printer settings were dropped: {why}")),
            }
        }
        // Header and footer pictures (`&G`): a VML drawing of their own and the images it shows,
        // kept byte for byte (Cella writes the header text, not the pictures).
        if let Some(id) = scan.legacy_drawing_hf.as_deref()
            && scan.legacy_drawing.as_deref() != Some(id)
            && let Some(r) = rels.iter().find(|r| r.id == id && is_kind(r, &["vmlDrawing"]))
        {
            match self.keep_through(cx, p, std::slice::from_ref(r), seen, &["vmlDrawing"]) {
                Ok(()) => {
                    let xml = format!("<legacyDrawingHF xmlns:r=\"{NS_REL}\" r:id=\"{}\"/>", esc_attr(&r.id));
                    p.elements.push(PreservedXml { slot: "legacyDrawingHF".into(), xml });
                    cx.warn(format!("sheet \"{name}\": pictures in the header and footer are not shown; they are kept for saving"));
                }
                Err(why) => cx.warn(format!("sheet \"{name}\": header and footer pictures were dropped: {why}")),
            }
        }
        // Form controls the sheet reader modelled are written from the model; the others stay.
        let modelled = cx.controls_modelled.clone();
        let had_controls = scan.fragments.iter().any(|(slot, _)| slot == "controls");
        let mut frags: Vec<(String, String)> = scan
            .fragments
            .into_iter()
            .filter_map(|(slot, x)| if slot == "controls" { without_controls(&x, &modelled).map(|x| (slot, x)) } else { Some((slot, x)) })
            .collect();
        // Controls and OLE objects are drawn from shapes in the legacy VML drawing. Controls only
        // the drawing lists need it too, whether or not a `<controls>` element is left.
        if frags.iter().any(|(slot, _)| slot != "picture") {
            match self.vml(cx, rels, scan.legacy_drawing.as_deref(), seen) {
                Ok((vml, parts)) => {
                    p.parts.extend(parts);
                    p.vml = Some(vml);
                }
                Err(why) => {
                    cx.warn(format!("sheet \"{name}\": controls and embedded objects were dropped: {why}"));
                    frags.retain(|(slot, _)| slot == "picture");
                }
            }
        }
        for (slot, x) in frags {
            let mut ids = vec![];
            let ok = map_rel_ids(&x, &mut |id| {
                ids.push(id.to_string());
                Some(id.to_string())
            })
            .is_some();
            let roots: Option<Vec<PreservedRel>> = ids.iter().map(|id| rels.iter().find(|r| r.id == *id).cloned()).collect();
            let kept = match (ok, roots) {
                (true, Some(roots)) => self.keep(cx, p, &roots, seen),
                (false, _) => Err("its XML is damaged".into()),
                (_, None) => Err("it refers to a missing relationship".into()),
            };
            match kept {
                Ok(()) => p.elements.push(PreservedXml { slot, xml: x }),
                Err(why) => cx.warn(format!("sheet \"{name}\": {slot} were dropped: {why}")),
            }
        }
        // A drawing whose `<controls>` entry is gone is still kept when it holds a control only it
        // lists; a sheet that never listed any keeps none, as before.
        let keep_vml =
            p.elements.iter().any(|e| e.slot == "oleObjects" || e.slot == "controls") || (had_controls && cx.vml_only_unmodelled && p.vml.is_some());
        if keep_vml {
            cx.warn(format!("sheet \"{name}\": embedded objects and form controls are not shown; they are kept for saving"));
        } else {
            p.vml = None;
        }
    }

    /// The sheet's VML drawing without its notes and the form controls read into the model
    /// (Cella writes those), and the parts its shapes refer to (preview images).
    fn vml(
        &mut self,
        cx: &mut Ctx<'_>,
        rels: &[PreservedRel],
        rid: Option<&str>,
        seen: &mut HashSet<String>,
    ) -> Result<(PreservedPart, Vec<PreservedPart>), String> {
        let rel = rid.and_then(|id| rels.iter().find(|r| r.id == id && !r.external)).ok_or("the sheet has no VML drawing")?;
        // Charged to the kept total like any other part: many sheets can point at one highly
        // compressed VML drawing, and each keeps its own copy.
        if self.budget == 0 {
            return Err("it is too large".into());
        }
        let limit = MAX_META.min(u64::try_from(self.budget).unwrap_or(u64::MAX));
        let data = match cx.pkg.read_limited(&rel.target, limit) {
            Ok(Some(d)) => d,
            Ok(None) => return Err(format!("{} is missing", rel.target)),
            Err(IoError::TooLarge(_)) => return Err("it is too large".into()),
            Err(e) => return Err(e.to_string()),
        };
        self.budget = self.budget.saturating_sub(data.len());
        let text = String::from_utf8(data).map_err(|_| "the VML drawing is not UTF-8".to_string())?;
        let text = strip_shapes(&text, &cx.controls_modelled);
        let vrels = raw_rels(cx, &rel.target)?;
        let mut q = Preserved::default();
        let roots: Vec<PreservedRel> = vrels.iter().filter(|r| !r.external).cloned().collect();
        self.keep(cx, &mut q, &roots, seen)?;
        let vml = PreservedPart { name: rel.target.clone(), content_type: CT_VML.into(), data: Arc::from(text.into_bytes()), rels: vrels };
        Ok((vml, q.parts))
    }
}
/// What a sheet part holds for pass-through.
struct SheetScan {
    /// (slot, XML with the namespace declarations it needs).
    fragments: Vec<(String, String)>,
    /// The raw `<ext>` children of the sheet's `<extLst>`.
    exts: Vec<String>,
    ext_bytes: usize,
    legacy_drawing: Option<String>,
    /// The relationship id on `<legacyDrawingHF>` (header and footer pictures).
    legacy_drawing_hf: Option<String>,
    /// The relationship id on `<pageSetup>` (printer settings).
    page_setup: Option<String>,
}
/// The slot prefix of a chart sheet's kept children (`chartSheet:pageSetup`), so they cannot be
/// confused with a worksheet's kept sheet elements.
pub(crate) const CHART_SHEET_SLOT: &str = "chartSheet:";
/// One kept child of a chart sheet part: its element name, its XML with the namespace declarations
/// it needs, and the relationship ids it names.
type ChartSheetChild = (String, String, Vec<String>);
/// A chart sheet's top-level children, in document order: everything the model has no room for
/// (page margins, page setup, header and footer, protection, extensions) is written back around the
/// chart the model does keep. `None` when the part is damaged.
fn scan_chart_sheet(bytes: &[u8]) -> Option<Vec<ChartSheetChild>> {
    let data = xml::to_utf8(bytes);
    let text = std::str::from_utf8(&data).ok()?;
    let mut reader = Reader::from_str(text);
    let mut decls: Vec<(String, String)> = vec![];
    let mut out: Vec<ChartSheetChild> = vec![];
    let mut in_root = false;
    loop {
        let start = usize::try_from(reader.buffer_position()).ok()?;
        match reader.read_event().ok()? {
            Event::Eof => break,
            Event::Start(e) if !in_root => {
                in_root = true;
                for a in e.attributes().with_checks(false).flatten() {
                    if let Some(p) = a.key.as_ref().strip_prefix(b"xmlns:")
                        && let (Ok(p), Ok(v)) = (std::str::from_utf8(p), std::str::from_utf8(&a.value))
                        && !v.contains('"')
                    {
                        decls.push((p.to_string(), v.to_string()));
                    }
                }
            }
            Event::Start(e) if in_root => {
                let tag = String::from_utf8_lossy(e.local_name().as_ref()).into_owned();
                reader.read_to_end(e.name()).ok()?;
                let end = usize::try_from(reader.buffer_position()).ok()?;
                out.push(chart_sheet_child(text, start, end, &decls, tag)?);
            }
            Event::Empty(e) if in_root => {
                let tag = String::from_utf8_lossy(e.local_name().as_ref()).into_owned();
                let end = usize::try_from(reader.buffer_position()).ok()?;
                out.push(chart_sheet_child(text, start, end, &decls, tag)?);
            }
            _ => {}
        }
    }
    Some(out)
}
fn chart_sheet_child(text: &str, start: usize, end: usize, decls: &[(String, String)], tag: String) -> Option<ChartSheetChild> {
    if end.saturating_sub(start) > MAX_FRAGMENT {
        return None;
    }
    let frag = with_decls(text.get(start..end)?, decls);
    let ids = rel_ids(&frag)?;
    Some((tag, frag, ids))
}
/// Finds the sheet's `<picture>`, `<oleObjects>`, `<controls>` (also inside
/// `<mc:AlternateContent>`) and the `<legacyDrawing>` and `<legacyDrawingHF>` ids. `None` when the XML is damaged.
fn scan_sheet(bytes: &[u8]) -> Option<SheetScan> {
    let data = xml::to_utf8(bytes);
    let text = std::str::from_utf8(&data).ok()?;
    let mut reader = Reader::from_str(text);
    let mut decls: Vec<(String, String)> = vec![];
    let mut out = SheetScan { fragments: vec![], exts: vec![], ext_bytes: 0, legacy_drawing: None, legacy_drawing_hf: None, page_setup: None };
    let mut in_root = false;
    loop {
        let start = usize::try_from(reader.buffer_position()).ok()?;
        match reader.read_event().ok()? {
            Event::Eof => break,
            Event::Start(e) if !in_root => {
                in_root = true;
                for a in e.attributes().with_checks(false).flatten() {
                    if let Some(p) = a.key.as_ref().strip_prefix(b"xmlns:")
                        && let (Ok(p), Ok(v)) = (std::str::from_utf8(p), std::str::from_utf8(&a.value))
                        && !v.contains('"')
                    {
                        decls.push((p.to_string(), v.to_string()));
                    }
                }
            }
            Event::Start(e) => {
                let local = e.local_name().as_ref().to_vec();
                match local.as_slice() {
                    b"pageSetup" => out.page_setup = rel_id_attr(&e),
                    b"legacyDrawing" => out.legacy_drawing = rel_id_attr(&e),
                    b"legacyDrawingHF" => out.legacy_drawing_hf = rel_id_attr(&e),
                    b"extLst" => {
                        read_ext_list(&mut reader, text, &decls, &mut out.exts, &mut out.ext_bytes)?;
                        continue;
                    }
                    _ => {}
                }
                reader.read_to_end(e.name()).ok()?;
                let end = usize::try_from(reader.buffer_position()).ok()?;
                let wanted = matches!(local.as_slice(), b"picture" | b"oleObjects" | b"controls" | b"AlternateContent");
                if wanted && end.saturating_sub(start) <= MAX_FRAGMENT {
                    let frag = text.get(start..end)?;
                    push_fragment(&mut out, frag, &local, &decls);
                }
            }
            Event::Empty(e) if in_root => {
                let local = e.local_name().as_ref().to_vec();
                if local == b"legacyDrawing" || local == b"legacyDrawingHF" || local == b"pageSetup" {
                    let id = rel_id_attr(&e);
                    match local.as_slice() {
                        b"pageSetup" => out.page_setup = id,
                        b"legacyDrawing" => out.legacy_drawing = id,
                        _ => out.legacy_drawing_hf = id,
                    }
                } else if local == b"picture"
                    && let Some(frag) = text.get(start..usize::try_from(reader.buffer_position()).ok()?)
                {
                    push_fragment(&mut out, frag, &local, &decls);
                }
            }
            Event::End(_) => break,
            _ => {}
        }
    }
    Some(out)
}
/// The `r:id` of an element (an `id` attribute with a namespace prefix).
fn rel_id_attr(e: &BytesStart<'_>) -> Option<String> {
    e.attributes()
        .with_checks(false)
        .flatten()
        .find(|a| a.key.local_name().as_ref() == b"id" && a.key.prefix().is_some())
        .and_then(|a| String::from_utf8(a.value.to_vec()).ok())
}
fn push_fragment(out: &mut SheetScan, frag: &str, local: &[u8], decls: &[(String, String)]) {
    let frag = with_decls(frag, decls);
    let slot = if local == b"AlternateContent" {
        // The kept element is the one the first choice holds.
        let Ok(el) = xml::parse(frag.as_bytes()) else { return };
        match el.child("Choice").and_then(|c| c.children.first()) {
            Some(c) => c.name.clone(),
            None => return,
        }
    } else {
        String::from_utf8_lossy(local).into_owned()
    };
    if SHEET_SLOTS.contains(&slot.as_str()) && !out.fragments.iter().any(|(s, _)| *s == slot) {
        out.fragments.push((slot, frag));
    }
}
/// Adds the root's namespace declarations a fragment doesn't declare itself.
fn with_decls(frag: &str, decls: &[(String, String)]) -> String {
    let Some(name_end) = frag.char_indices().skip(1).find(|(_, c)| c.is_whitespace() || *c == '/' || *c == '>').map(|(i, _)| i) else {
        return frag.to_string();
    };
    let head = frag.get(..frag.find('>').unwrap_or(frag.len())).unwrap_or("");
    let mut add = String::new();
    for (p, v) in decls {
        if !head.contains(&format!("xmlns:{p}=")) {
            let _ = write!(add, " xmlns:{p}=\"{v}\"");
        }
    }
    format!("{}{add}{}", frag.get(..name_end).unwrap_or(""), frag.get(name_end..).unwrap_or(""))
}
/// Byte ranges of the `<v:shape …>…</v:shape>` (or self-closed) elements in VML text.
fn shape_spans(vml: &str) -> Vec<(usize, usize)> {
    let mut spans = vec![];
    let mut from = 0;
    while let Some(i) = vml.get(from..).and_then(|s| s.find("<v:shape")).map(|i| i + from) {
        let after = i + "<v:shape".len();
        let is_shape = vml.get(after..).and_then(|s| s.chars().next()).is_some_and(|c| c.is_whitespace() || c == '>' || c == '/');
        if !is_shape {
            from = after;
            continue;
        }
        let Some(tag_end) = vml.get(i..).and_then(|s| s.find('>')).map(|j| i + j + 1) else { break };
        let end = if vml.get(..tag_end).is_some_and(|t| t.ends_with("/>")) {
            tag_end
        } else {
            match vml.get(tag_end..).and_then(|s| s.find("</v:shape>")) {
                Some(j) => tag_end + j + "</v:shape>".len(),
                None => break,
            }
        };
        spans.push((i, end));
        from = end;
    }
    spans
}
/// The first start tag named `local` in XML text `x`, parsed on its own so an unclosed element
/// before it (a caption's `<br>`) doesn't hide it.
fn first_start(x: &str, local: &str) -> Option<crate::xml::El> {
    let mut from = 0;
    while let Some(i) = x.get(from..).and_then(|s| s.find('<')).map(|i| i + from) {
        from = i + 1;
        let rest = x.get(from..)?;
        let end = rest.find(|c: char| c.is_whitespace() || c == '>' || c == '/')?;
        if rest.get(..end).map(local_name) != Some(local) {
            continue;
        }
        let tag_end = x.get(i..)?.find('>')? + i + 1;
        if let Ok(el) = crate::xml::parse(x.get(i..tag_end)?.as_bytes()) {
            return Some(el);
        }
    }
    None
}
/// VML without its note shapes (Cella writes those) and without the shapes numbered in
/// `drop` (the form controls the sheet reader modelled). Each shape is judged by its own client
/// data and tags, never by text elsewhere in it: a button captioned `ObjectType="Note"` is a
/// button, and a shape's number is its `id`, else its `o:spid` ([\`crate::controls::number_of\`]).
/// A shape that doesn't parse is kept.
fn strip_shapes(vml: &str, drop: &HashSet<u64>) -> String {
    let mut out = String::with_capacity(vml.len());
    let mut last = 0;
    for (a, b) in shape_spans(vml) {
        let shape = vml.get(a..b).unwrap_or("");
        // Each tag is read on its own: a caption's unclosed `<br>` must not hide the client data
        // or the number that follow it.
        let note = first_start(shape, "ClientData").is_some_and(|d| crate::controls::is_note_data(&d));
        let numbered = first_start(shape, "shape").and_then(|s| crate::controls::number_of(&s)).is_some_and(|n| drop.contains(&n));
        if note || numbered {
            out.push_str(vml.get(last..a).unwrap_or(""));
            last = b;
        }
    }
    out.push_str(vml.get(last..).unwrap_or(""));
    out
}
/// Shape numbers (`_x0000_s1025` → 1025) used in VML text.
fn shape_numbers(vml: &str) -> Vec<u64> {
    vml.match_indices("_x0000_s")
        .filter_map(|(i, m)| {
            let digits: String = vml.get(i + m.len()..)?.chars().take_while(char::is_ascii_digit).take(18).collect();
            digits.parse().ok()
        })
        .collect()
}
/// The highest shape block VML text claims in `<o:idmap data>` or uses in a shape id.
pub(crate) fn vml_block_max(vml: &str) -> u64 {
    let used = shape_numbers(vml).into_iter().max().unwrap_or(0) / 1024;
    let claimed = vml
        .find("<o:idmap")
        .and_then(|i| vml.get(i..)?.find("data=\"").map(|d| i + d + "data=\"".len()))
        .and_then(|d| Some((d, d + vml.get(d..)?.find('"')?)))
        .and_then(|(d, e)| vml.get(d..e))
        .map(|list| list.split(',').filter_map(|b| b.trim().parse::<u64>().ok()).max().unwrap_or(0))
        .unwrap_or(0);
    used.max(claimed)
}
/// The first shape block free in kept VML: Cella's own shapes (notes, then form controls) are
/// numbered from `block * 1024 + 1` when they join it ([\`merge_vml\`]).
pub(crate) fn first_free_block(kept: &str) -> u64 {
    shape_numbers(kept).into_iter().max().unwrap_or(0) / 1024 + 1
}
/// Kept VML (controls, OLE objects) plus Cella's note and form control shapes: ours get ids
/// in fresh shape blocks so they can't collide with the kept shapes the sheet XML refers to.
pub(crate) fn merge_vml(kept: &str, ours: &str) -> String {
    let Some(close) = kept.rfind("</xml>") else { return ours.to_string() };
    let block = first_free_block(kept);
    let mut add = String::new();
    // Our shape types (notes, form controls) that the kept drawing doesn't define.
    let mut from = 0;
    while let Some(i) = ours.get(from..).and_then(|s| s.find("<v:shapetype")).map(|i| i + from) {
        let Some(j) = ours.get(i..).and_then(|s| s.find("</v:shapetype>")).map(|j| i + j + "</v:shapetype>".len()) else { break };
        let ty = ours.get(i..j).unwrap_or("");
        let id = attr_value(ty, "id").unwrap_or("");
        if !id.is_empty() && !kept.contains(&format!("id=\"{id}\"")) && !kept.contains(&format!("id='{id}'")) {
            add.push_str(ty);
        }
        from = j;
    }
    let spans = shape_spans(ours);
    let blocks = (spans.len() as u64 + 1).div_ceil(1024).max(1);
    for (k, (a, b)) in spans.into_iter().enumerate() {
        let shape = ours.get(a..b).unwrap_or("");
        let id = block.saturating_mul(1024).saturating_add(1 + k as u64);
        match (shape.find("id=\"_x0000_s"), shape.find("id=\"_x0000_s").and_then(|i| shape.get(i + 4..)?.find('"').map(|j| i + 4 + j))) {
            (Some(i), Some(j)) => {
                let _ = write!(add, "{}id=\"_x0000_s{id}\"{}", shape.get(..i).unwrap_or(""), shape.get(j + 1..).unwrap_or(""));
            }
            _ => add.push_str(shape),
        }
    }
    let mut head = kept.get(..close).unwrap_or("").to_string();
    // Claim the blocks in the drawing's id map.
    if let Some(i) = head.find("<o:idmap")
        && let Some(d) = head.get(i..).and_then(|s| s.find("data=\"")).map(|d| i + d + "data=\"".len())
        && let Some(e) = head.get(d..).and_then(|s| s.find('"')).map(|e| d + e)
    {
        let claimed: String = (block..block.saturating_add(blocks)).map(|b| format!(",{b}")).collect();
        head.insert_str(e, &claimed);
    }
    format!("{head}{add}{}", kept.get(close..).unwrap_or(""))
}
/// The value of the first attribute `name` in XML text `x`, in either quote style (`name="v"` or
/// `name='v'`), when it has one. The name must start a token (`id` is not the end of `spid`).
fn attr_value<'a>(x: &'a str, name: &str) -> Option<&'a str> {
    let mut from = 0;
    while let Some(i) = x.get(from..)?.find(name).map(|i| i + from) {
        from = i + name.len();
        let starts_token = x.get(..i).is_some_and(|before| before.ends_with(|c: char| c.is_whitespace() || c == '<'));
        let Some(rest) = x.get(from..).map(str::trim_start).and_then(|r| r.strip_prefix('=')).map(str::trim_start) else { continue };
        if !starts_token {
            continue;
        }
        if let Some(q) = rest.chars().next().filter(|q| *q == '"' || *q == '\'') {
            let body = rest.get(q.len_utf8()..)?;
            return body.find(q).and_then(|e| body.get(..e));
        }
    }
    None
}
/// A kept `<controls>` fragment without the controls whose `shapeId` is in `drop` (they are
/// written from the model): a bare `<control>` child goes when it is one of them, and within an
/// `mc:AlternateContent` child only those controls go, in each of its branches, the wrapper
/// staying while a control remains. `None` when nothing is left. A fragment that doesn't parse
/// is returned unchanged.
fn without_controls(frag: &str, drop: &HashSet<u64>) -> Option<String> {
    if drop.is_empty() {
        return Some(frag.to_string());
    }
    let mut reader = Reader::from_str(frag);
    let mut out = String::with_capacity(frag.len());
    let (mut last, mut left, mut inside) = (0usize, 0usize, false);
    loop {
        let start = usize::try_from(reader.buffer_position()).unwrap_or(0);
        let ev = match reader.read_event() {
            Ok(ev) => ev,
            Err(_) => return Some(frag.to_string()),
        };
        let child = match ev {
            Event::Eof => break,
            Event::Start(e) if e.local_name().as_ref() == b"controls" => {
                inside = true;
                continue;
            }
            Event::End(e) if e.local_name().as_ref() == b"controls" => {
                inside = false;
                continue;
            }
            Event::Start(e) if inside => {
                if reader.read_to_end(e.name()).is_err() {
                    return Some(frag.to_string());
                }
                true
            }
            Event::Empty(_) if inside => true,
            _ => false,
        };
        if !child {
            continue;
        }
        let end = usize::try_from(reader.buffer_position()).unwrap_or(start);
        let text = frag.get(start..end).unwrap_or("");
        let Some((kept, controls, dropped)) = controls_kept(text, drop) else { return Some(frag.to_string()) };
        if dropped == 0 {
            // Nothing of ours in it: the child stays as it is.
            left += 1;
        } else {
            out.push_str(frag.get(last..start).unwrap_or(""));
            if controls > 0 {
                out.push_str(&kept);
                left += 1;
            }
            last = end;
        }
    }
    out.push_str(frag.get(last..).unwrap_or(""));
    (left > 0).then_some(out)
}
/// One child of a `<controls>` element with the `<control>` elements numbered in `drop` taken
/// out, wherever they are in it (a bare control, or the Choice and Fallback branches of an
/// `mc:AlternateContent`), as (the text, the controls that remain, the controls taken out). The
/// `shapeId` is read as the control reader reads it (an attribute, in either quote style). `None`
/// when the child doesn't parse.
fn controls_kept(child: &str, drop: &HashSet<u64>) -> Option<(String, usize, usize)> {
    let mut reader = Reader::from_str(child);
    let mut out = String::with_capacity(child.len());
    let (mut last, mut kept, mut dropped) = (0usize, 0usize, 0usize);
    loop {
        let start = usize::try_from(reader.buffer_position()).ok()?;
        let (e, has_end) = match reader.read_event().ok()? {
            Event::Eof => break,
            Event::Start(e) => (e, true),
            Event::Empty(e) => (e, false),
            _ => continue,
        };
        if e.local_name().as_ref() != b"control" {
            // A wrapper or a branch: its controls are the events that follow.
            continue;
        }
        let number =
            e.try_get_attribute("shapeId").ok().flatten().and_then(|a| std::str::from_utf8(&a.value).ok().and_then(crate::controls::shape_number));
        if has_end {
            reader.read_to_end(e.name()).ok()?;
        }
        let end = usize::try_from(reader.buffer_position()).ok()?;
        if number.is_some_and(|n| drop.contains(&n)) {
            out.push_str(child.get(last..start)?);
            last = end;
            dropped += 1;
        } else {
            kept += 1;
        }
    }
    out.push_str(child.get(last..)?);
    Some((out, kept, dropped))
}
/// Rewrites relationship-id attributes (`r:id`, `r:embed`…: any attribute in the relationships
/// namespace) of an XML fragment through `map`. `None` when the fragment is damaged or `map` has
/// no id for one of them.
pub fn map_rel_ids(x: &str, map: &mut dyn FnMut(&str) -> Option<String>) -> Option<String> {
    let mut reader = Reader::from_str(x);
    let mut writer = Writer::new(Vec::with_capacity(x.len()));
    let mut prefixes: HashSet<Vec<u8>> = HashSet::new();
    let mut depth = 0usize;
    loop {
        match reader.read_event().ok()? {
            Event::Eof => break,
            Event::Start(e) => {
                depth += 1;
                if depth > MAX_FRAGMENT_DEPTH {
                    return None;
                }
                let e = map_start(&e, &mut prefixes, map)?;
                writer.write_event(Event::Start(e)).ok()?;
            }
            Event::Empty(e) => {
                let e = map_start(&e, &mut prefixes, map)?;
                writer.write_event(Event::Empty(e)).ok()?;
            }
            Event::End(e) => {
                depth = depth.saturating_sub(1);
                writer.write_event(Event::End(e)).ok()?;
            }
            ev => writer.write_event(ev).ok()?,
        }
    }
    String::from_utf8(writer.into_inner()).ok()
}
fn map_start(e: &BytesStart<'_>, prefixes: &mut HashSet<Vec<u8>>, map: &mut dyn FnMut(&str) -> Option<String>) -> Option<BytesStart<'static>> {
    for a in e.attributes().with_checks(false) {
        let a = a.ok()?;
        if let Some(p) = a.key.as_ref().strip_prefix(b"xmlns:")
            && (a.value.as_ref() == NS_REL.as_bytes() || a.value.as_ref() == NS_REL_STRICT.as_bytes())
        {
            prefixes.insert(p.to_vec());
        }
    }
    let name = String::from_utf8(e.name().as_ref().to_vec()).ok()?;
    let mut out = BytesStart::new(name);
    for a in e.attributes().with_checks(false) {
        let a = a.ok()?;
        let key = a.key.as_ref();
        let is_rel = !key.starts_with(b"xmlns") && a.key.prefix().is_some_and(|p| prefixes.contains(p.as_ref()));
        if is_rel {
            let old = std::str::from_utf8(&a.value).ok()?;
            let new = map(old)?;
            out.push_attribute(Attribute { key: QName(key), value: Cow::Owned(esc_attr(&new).into_bytes()) });
        } else {
            out.push_attribute(Attribute { key: QName(key), value: a.value.clone() });
        }
    }
    Some(out)
}
