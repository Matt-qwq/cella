//! Writing kept parts back with fresh part names and relationship ids.

use super::read::*;
use super::*;

// ---------------------------------------------------------------- writing

/// What the workbook still has, for the check a kept part has to pass: a slicer cache names the
/// PivotTables it filters, a slicer or timeline names its cache.
#[derive(Default)]
pub(crate) struct PartFacts {
    pivots: HashSet<String>,
    caches: HashSet<String>,
    /// Keep every part, whatever it names. Content carried for a PivotTable GridCraft does not
    /// compute is never repaired: the parts are written as they came, and a part dropped behind
    /// one of them would leave the relationship ids its own bytes name pointing at nothing.
    keep_all: bool,
}
/// Names given to kept parts while a package is written.
#[derive(Default)]
pub struct KeptOut {
    used: HashSet<String>,
    n: u32,
    /// Every part written by name (normalized original → written name), across all owners: a part
    /// reached from two owners is written once and related twice.
    names: NameMap,
    /// Part names (normalized) already written, so a part two owners keep is one zip entry.
    written: HashSet<String>,
    /// The PivotTable and cache names the workbook still has (see [`part_usable`]).
    pub(crate) facts: PartFacts,
    /// The highest VML shape block a kept header and footer drawing uses: notes go after it.
    vml_blocks: u32,
    /// Package-root relationships (type, target, external), added when `_rels/.rels` is written.
    root: Vec<(String, String, bool)>,
}
impl KeptOut {
    /// Reserves a part name GridCraft writes itself, so kept parts don't take it. False when it is
    /// already taken.
    pub fn reserve(&mut self, name: &str) -> bool {
        self.used.insert(name.to_ascii_lowercase())
    }

    /// Records the PivotTables and caches the kept content may refer to, so a part that names a
    /// deleted or renamed owner is dropped instead of written pointing at something the workbook
    /// no longer has.
    pub fn set_workbook_facts(&mut self, wb: &Workbook) {
        self.facts.pivots = wb.sheets.iter().flat_map(|s| s.pivots.iter()).map(|p| p.name.to_lowercase()).collect();
        let caches = wb.preserved.iter().map(|p| &**p).chain(wb.sheets.iter().filter_map(|s| s.preserved.as_deref())).flat_map(|p| &p.parts);
        let names: Vec<String> = caches
            .filter(|q| is_cache_part(q) && pivot_names_ok(&self.facts.pivots, q))
            .filter_map(|q| xml::parse(&q.data).ok().and_then(|x| x.attr("name").map(|n| n.to_lowercase())))
            .collect();
        self.facts.caches = names.into_iter().collect();
    }

    /// Reserves the VML shape blocks the kept header and footer drawings of `wb` use, so that the
    /// notes drawings GridCraft writes don't reuse their shape ids.
    pub fn reserve_vml_blocks(&mut self, wb: &Workbook) {
        let vmls =
            wb.sheets.iter().filter_map(|s| s.preserved.as_deref()).flat_map(|p| &p.parts).filter(|q| q.content_type.eq_ignore_ascii_case(CT_VML));
        let max = vmls.map(|q| vml_block_max(&String::from_utf8_lossy(&q.data))).max().unwrap_or(0);
        self.vml_blocks = u32::try_from(max).unwrap_or(u32::MAX);
    }

    /// The shape block of the `n`th notes drawing.
    pub fn notes_block(&self, n: u32) -> u32 {
        n.saturating_add(self.vml_blocks)
    }

    /// Gives the kept part `original` the name `preferred` when nothing has taken it, else the
    /// name [`Self::claim`] would give it. Idempotent: a part reached from two owners keeps the
    /// first name it was given.
    pub fn name_as(&mut self, original: &str, preferred: &str) -> String {
        let key = normalize(original);
        if let Some(n) = self.names.get(&key) {
            return n.clone();
        }
        let name = if self.used.insert(preferred.to_ascii_lowercase()) { preferred.to_string() } else { self.claim(original) };
        self.names.insert(key, name.clone());
        name
    }

    /// A part name for a kept part: its original name when that can't collide with anything
    /// written, otherwise a fresh one in the same folder.
    pub(crate) fn claim(&mut self, original: &str) -> String {
        let lower = original.to_ascii_lowercase();
        let own = OWN_DIRS.iter().any(|d| lower.starts_with(d));
        if own && self.used.insert(lower) {
            return original.to_string();
        }
        let (dir, file) = package::split_dir(original);
        let (stem, ext) = match file.rsplit_once('.') {
            Some((s, e)) => (s, format!(".{e}")),
            None => (file, String::new()),
        };
        let stem = if own { format!("{stem}_") } else { "kept".to_string() };
        loop {
            self.n = self.n.saturating_add(1);
            let file = format!("{stem}{}{ext}", self.n);
            let cand = if dir.is_empty() { file } else { format!("{dir}/{file}") };
            if self.used.insert(cand.to_ascii_lowercase()) || self.n == u32::MAX {
                return cand;
            }
        }
    }
}
/// Original part name (normalized) → written part name.
pub(crate) type NameMap = HashMap<String, String>;
fn rel_target(r: &PreservedRel, names: &NameMap) -> Option<String> {
    if r.external { Some(r.target.clone()) } else { names.get(&normalize(&r.target)).map(|n| format!("/{n}")) }
}
/// A kept part's relationships, ids unchanged, targets renamed.
pub(crate) fn part_rels_xml(rels: &[PreservedRel], names: &NameMap) -> String {
    let mut s = format!("{XML_DECL}<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">");
    for r in rels {
        let Some(target) = rel_target(r, names) else { continue };
        let _ = write!(
            s,
            "<Relationship Id=\"{}\" Type=\"{}\" Target=\"{}\"{}/>",
            esc_attr(&r.id),
            esc_attr(&r.rel_type),
            esc_attr(&target),
            if r.external { " TargetMode=\"External\"" } else { "" }
        );
    }
    s.push_str("</Relationships>");
    s
}
pub(crate) fn write_parts(out: &mut Out, parts: &[PreservedPart]) -> NameMap {
    let facts = std::mem::take(&mut out.kept.facts);
    let usable: Vec<&PreservedPart> = parts.iter().filter(|q| part_usable(&facts, q)).collect();
    let mut names = NameMap::new();
    for p in &usable {
        let key = normalize(&p.name);
        let name = match out.kept.names.get(&key) {
            Some(n) => n.clone(),
            None => {
                let n = out.kept.claim(&p.name);
                out.kept.names.insert(key.clone(), n.clone());
                n
            }
        };
        names.insert(key, name);
    }
    let mut written = HashSet::new();
    for p in &usable {
        let Some(new) = names.get(&normalize(&p.name)).cloned() else { continue };
        if !written.insert(new.clone()) || !out.kept.written.insert(new.to_ascii_lowercase()) {
            continue;
        }
        out.part(&new, Some(&p.content_type), p.data.to_vec());
        if !p.rels.is_empty() {
            out.part(&rels_part_name(&new), None, part_rels_xml(&p.rels, &names).into_bytes());
        }
    }
    out.kept.facts = facts;
    names
}
fn is_cache_part(p: &PreservedPart) -> bool {
    p.content_type.eq_ignore_ascii_case(CT_SLICER_CACHE) || p.content_type.eq_ignore_ascii_case(CT_TIMELINE_CACHE)
}
/// Whether a slicer or timeline cache still names the PivotTables the workbook has.
fn pivot_names_ok(pivots: &HashSet<String>, p: &PreservedPart) -> bool {
    let Ok(x) = xml::parse(&p.data) else { return false };
    let names: Vec<&str> = x.child("pivotTables").map(|l| l.kids("pivotTable").filter_map(|q| q.attr("name")).collect()).unwrap_or_default();
    names.iter().all(|n| n.trim().is_empty() || pivots.contains(&n.to_lowercase()))
}
/// Whether a kept part can still be written. A slicer or timeline cache names the PivotTables it
/// filters, and a slicer or timeline names its cache; an owner the workbook no longer has (deleted,
/// renamed) would leave the part pointing at nothing. The part goes, and with it whatever only it
/// reaches — no relationship to a part that isn't written is ever left behind.
fn part_usable(facts: &PartFacts, p: &PreservedPart) -> bool {
    if facts.keep_all {
        return true;
    }
    if is_cache_part(p) {
        return pivot_names_ok(&facts.pivots, p);
    }
    let slicer = p.content_type.eq_ignore_ascii_case(CT_SLICER) || p.content_type.eq_ignore_ascii_case(CT_TIMELINE);
    if !slicer {
        return true;
    }
    let Ok(x) = xml::parse(&p.data) else { return false };
    let names: Vec<&str> = x.children.iter().filter(|c| c.name == "slicer" || c.name == "timeline").filter_map(|c| c.attr("cache")).collect();
    names.iter().all(|n| n.trim().is_empty() || facts.caches.contains(&n.to_lowercase()))
}
/// The parts and relationships [`Keep::sheet`] kept for a PivotTable GridCraft does not compute
/// (its own part, its cache definition, its records, the data model behind them).
pub fn owner_kept(owner: Option<&Preserved>, key: Option<&str>) -> (Vec<PreservedPart>, Vec<PreservedRel>) {
    let (Some(p), Some(key)) = (owner, key) else { return (vec![], vec![]) };
    let (parts, sources, _frags) = owner_parts(p, key, &PartFacts { keep_all: true, ..PartFacts::default() });
    (parts, sources)
}
/// Writes the parts of a PivotTable GridCraft does not compute, under the names `name_of` gives
/// them (the writer's own: a kept report keeps its `pivotTableN.xml` numbering and its cache the
/// id it was written with, so the report's `cacheId` still names its cache, and every part comes
/// back byte for byte). Returns the kept relationships with their targets rewritten to the written
/// parts: the report's own relationship to its cache definition among them — and the name each
/// part was written under (original part name, normalized, to written name). Both are empty when
/// the report kept nothing.
pub fn write_pivot_parts(
    owner: Option<&Preserved>,
    key: Option<&str>,
    out: &mut Out,
    name_of: &dyn Fn(&PreservedPart) -> String,
) -> (Vec<PreservedRel>, NameMap) {
    let (Some(p), Some(key)) = (owner, key) else { return (vec![], NameMap::new()) };
    // Nothing behind the report is dropped: its parts come back as they came.
    let keep_all = std::mem::replace(&mut out.kept.facts.keep_all, true);
    let (parts, sources, _frags) = owner_parts(p, key, &out.kept.facts);
    for q in &parts {
        out.kept.name_as(&q.name, &name_of(q));
    }
    let names = write_parts(out, &parts);
    out.kept.facts.keep_all = keep_all;
    let rels = sources
        .iter()
        .filter_map(|r| {
            let written = names.get(&normalize(&r.target))?.clone();
            Some(PreservedRel { id: r.id.clone(), rel_type: r.rel_type.clone(), target: written, external: r.external })
        })
        .collect();
    (rels, names)
}
/// Adds the owner's relationships; returns old id → new id.
pub(crate) fn attach(rels: &mut Rels, list: &[PreservedRel], names: &NameMap) -> HashMap<String, String> {
    let mut ids = HashMap::new();
    for r in list {
        if let Some(target) = rel_target(r, names) {
            let id = rels.add_raw(&r.rel_type, &target, r.external);
            ids.insert(r.id.clone(), id);
        }
    }
    ids
}
pub(crate) fn remap(x: &str, ids: &HashMap<String, String>) -> Option<String> {
    map_rel_ids(x, &mut |id| ids.get(id).cloned())
}
/// A kept chart sheet being written: its `<sheet>` entry's fields and its new relationship id.
pub struct KeptSheet {
    before: usize,
    name: String,
    visibility: Visibility,
    rid: String,
}
/// The workbook's kept content as written.
#[derive(Default)]
pub struct WorkbookKept {
    /// Workbook XML elements that go right after `<sheets>` (`<externalReferences>`).
    pub elements: String,
    /// The workbook `<ext>` children GridCraft does not model, relationship ids rewritten.
    pub ext: String,
    sheets: Vec<KeptSheet>,
}
impl WorkbookKept {
    /// Where the model's sheet `i` is in the file's sheet list: chart sheets come between worksheets.
    pub fn file_index(&self, i: usize) -> usize {
        i + self.sheets.iter().filter(|s| s.before <= i).count()
    }

    /// The `<sheet>` entries of the chart sheets that go right before the model's sheet `i`
    /// (`i` = the number of sheets for those that come last); `id` numbers them from `first_id`.
    pub fn sheet_entries(&self, i: usize, last: bool, mut id: usize) -> (String, usize) {
        let mut x = String::new();
        for s in self.sheets.iter().filter(|s| if last { s.before >= i } else { s.before == i }) {
            let state = match s.visibility {
                Visibility::Visible => "",
                Visibility::Hidden => " state=\"hidden\"",
                Visibility::VeryHidden => " state=\"veryHidden\"",
            };
            let name: String = s.name.chars().take(31).collect();
            let _ = write!(x, "<sheet name=\"{}\" sheetId=\"{id}\"{state} r:id=\"{}\"/>", esc_attr(&name), esc_attr(&s.rid));
            id += 1;
        }
        (x, id)
    }
}
/// The part names reachable from `root` through kept parts' relationships (normalized).
pub(crate) fn reachable(p: &Preserved, root: &PreservedRel) -> HashSet<String> {
    let by_name: HashMap<String, &PreservedPart> = p.parts.iter().map(|q| (normalize(&q.name), q)).collect();
    let mut seen = HashSet::new();
    let mut queue = VecDeque::from([normalize(&root.target)]);
    while let Some(n) = queue.pop_front() {
        if !seen.insert(n.clone()) {
            continue;
        }
        if let Some(q) = by_name.get(&n) {
            queue.extend(q.rels.iter().filter(|r| !r.external).map(|r| normalize(&r.target)));
        }
    }
    seen
}
/// The kept parts of `p` that one of `roots` reaches. Parts that only a rewritten owner's
/// relationships reach are written with that owner (`owner_extras`), so an owner that is gone
/// takes them with it instead of leaving an orphan part behind.
pub(crate) fn live_parts(p: &Preserved, roots: &[&PreservedRel], facts: &PartFacts) -> Vec<PreservedPart> {
    // Only usable parts are walked, so a part reached only through one that is dropped with its
    // owner (a slicer behind a dropped cache) is dropped too instead of being left orphaned.
    let by_name: HashMap<String, &PreservedPart> = p.parts.iter().filter(|q| part_usable(facts, q)).map(|q| (normalize(&q.name), q)).collect();
    let mut live: HashSet<String> = HashSet::new();
    let mut queue: VecDeque<String> = roots.iter().map(|r| normalize(&r.target)).collect();
    while let Some(n) = queue.pop_front() {
        if !live.insert(n.clone()) {
            continue;
        }
        if let Some(q) = by_name.get(&n) {
            queue.extend(q.rels.iter().filter(|r| !r.external).map(|r| normalize(&r.target)));
        }
    }
    p.parts.iter().filter(|q| live.contains(&normalize(&q.name)) && part_usable(facts, q)).cloned().collect()
}
/// The kept parts of `p` an owner GridCraft writes reaches through its own relationships.
fn owner_parts(p: &Preserved, key: &str, facts: &PartFacts) -> (Vec<PreservedPart>, Vec<PreservedRel>, Vec<String>) {
    let prefix = format!("{key}{KS}");
    let mut sources: Vec<PreservedRel> = vec![];
    for r in p.rels.iter().filter(|r| r.id.starts_with(&prefix)) {
        if let Some(id) = r.id.get(prefix.len()..) {
            sources.push(PreservedRel { id: id.to_string(), ..r.clone() });
        }
    }
    let frags: Vec<String> = p.elements.iter().filter(|e| e.slot == key).map(|e| e.xml.clone()).collect();
    let roots: Vec<&PreservedRel> = sources.iter().collect();
    (live_parts(p, &roots, facts), sources, frags)
}
/// What a part GridCraft rewrites keeps: its `<ext>` children and the relationships and parts
/// only it owns. Writes the parts and adds the relationships to `rels` (the part's own), then
/// returns the `<ext>` XML with fresh relationship ids; empty when `key` matches no kept owner
/// (the table or PivotTable was deleted or renamed) or it had nothing.
pub fn owner_extras(owner: Option<&Preserved>, key: Option<&str>, rels: &mut Rels, out: &mut Out) -> String {
    let (Some(p), Some(key)) = (owner, key) else { return String::new() };
    let facts = std::mem::take(&mut out.kept.facts);
    let (parts, sources, frags) = owner_parts(p, key, &facts);
    out.kept.facts = facts;
    if sources.is_empty() && frags.is_empty() {
        return String::new();
    }
    let names = write_parts(out, &parts);
    let ids = attach(rels, &sources, &names);
    let mut x = String::new();
    for f in &frags {
        if let Some(m) = remap(f, &ids) {
            x.push_str(&m);
        }
    }
    x
}
/// Every formula (`<c:f>`) of a chart part, depth first.
fn chart_formulas(el: &xml::El, out: &mut Vec<String>) {
    if el.name == "f" && !el.text.trim().is_empty() {
        out.push(el.text.trim().to_string());
    }
    for c in &el.children {
        chart_formulas(c, out);
    }
}
/// The sheet names a formula such as `'My Sheet'!$A$1:$A$9,Data!$B$1` refers to. References to
/// other workbooks (`[1]Sheet!A1`) are not sheets of this one and are left out.
pub(crate) fn sheet_names_in(formula: &str) -> Vec<String> {
    let mut names = vec![];
    let chars: Vec<char> = formula.chars().collect();
    let at = |i: usize| chars.get(i).copied();
    let mut i = 0;
    while let Some(c) = at(i) {
        let mut name = String::new();
        let start = i;
        if c == '\'' {
            i += 1;
            while let Some(c) = at(i) {
                if c == '\'' {
                    if at(i + 1) == Some('\'') {
                        name.push('\'');
                        i += 2;
                        continue;
                    }
                    break;
                }
                name.push(c);
                i += 1;
            }
            i += 1;
        } else {
            while let Some(c) = at(i).filter(|c| !matches!(c, '!' | ',' | '(' | ')' | '\'' | ' ')) {
                name.push(c);
                i += 1;
            }
        }
        if at(i) == Some('!') {
            if !name.is_empty() && !name.starts_with('[') {
                names.push(name);
            }
            i += 1;
        } else if i == start {
            i += 1;
        }
    }
    names
}
/// Whether a kept chart sheet can be written back: its chart's series must still refer to sheets
/// that exist under the same names (the chart is kept as it was, so a series on a deleted or
/// renamed sheet would be left pointing at nothing), and no worksheet may have taken its name.
fn chart_sheet_usable(wb: &Workbook, p: &Preserved, s: &PreservedSheet) -> bool {
    let same = |a: &str, b: &str| a.chars().take(31).collect::<String>().eq_ignore_ascii_case(&b.chars().take(31).collect::<String>());
    if wb.sheets.iter().any(|w| same(&w.name, &s.name)) {
        return false;
    }
    let Some(root) = p.rels.iter().find(|r| r.id == s.rel_id) else { return false };
    let parts = reachable(p, root);
    let mut formulas = vec![];
    for q in p.parts.iter().filter(|q| parts.contains(&normalize(&q.name)) && q.content_type.contains("chart")) {
        if let Ok(x) = xml::parse(&q.data) {
            chart_formulas(&x, &mut formulas);
        }
    }
    formulas.iter().flat_map(|f| sheet_names_in(f)).all(|n| wb.sheets.iter().any(|w| same(&w.name, &n)))
}
/// Writes the workbook's kept parts and relationships. Chart sheets that can't be written back
/// (see [`chart_sheet_usable`]) are left out with the parts only they reach.
pub fn write_workbook(wb: &Workbook, wb_rels: &mut Rels, out: &mut Out) -> WorkbookKept {
    let Some(p) = wb.preserved.as_deref() else { return WorkbookKept::default() };
    let (live, dropped): (Vec<&PreservedSheet>, Vec<&PreservedSheet>) = p.sheets.iter().partition(|s| chart_sheet_usable(wb, p, s));
    let root_of = |s: &PreservedSheet| p.rels.iter().find(|r| r.id == s.rel_id);
    let dropped_ids: HashSet<&str> = dropped.iter().map(|s| s.rel_id.as_str()).collect();
    // Parts reached only by dropped chart sheets go with them.
    let mut gone: HashSet<String> = dropped.iter().filter_map(|s| root_of(s)).flat_map(|r| reachable(p, r)).collect();
    if !gone.is_empty() {
        let others = p.rels.iter().filter(|r| !dropped_ids.contains(r.id.as_str())).flat_map(|r| reachable(p, r));
        for n in others {
            gone.remove(&n);
        }
    }
    let rels: Vec<PreservedRel> = p.rels.iter().filter(|r| !dropped_ids.contains(r.id.as_str()) && split_owner(&r.id).is_none()).cloned().collect();
    // A rewritten part's own parts (a PivotTable's slicer cache) are written with it.
    let facts = std::mem::take(&mut out.kept.facts);
    let mut roots: Vec<&PreservedRel> = rels.iter().collect();
    roots.extend(p.package_rels.iter());
    let mut kept_parts = live_parts(p, &roots, &facts);
    out.kept.facts = facts;
    kept_parts.retain(|q| !gone.contains(&normalize(&q.name)));
    let names = write_parts(out, &kept_parts);
    let ids = attach(wb_rels, &rels, &names);
    for r in &p.package_rels {
        if let Some(target) = rel_target(r, &names) {
            out.kept.root.push((r.rel_type.clone(), target, r.external));
        }
    }
    WorkbookKept {
        elements: p.elements.iter().filter(|e| e.slot == "externalReferences").filter_map(|e| remap(&e.xml, &ids)).collect(),
        ext: p.elements.iter().filter(|e| e.slot == EXT_SLOT).filter_map(|e| remap(&e.xml, &ids)).collect(),
        sheets: live
            .iter()
            .filter_map(|s| Some(KeptSheet { before: s.before, name: s.name.clone(), visibility: s.visibility, rid: ids.get(&s.rel_id)?.clone() }))
            .collect(),
    }
}
/// The workbook's kept elements of `slot` that refer to no part (`fileVersion`, the calc-features
/// extension).
pub fn workbook_elements(wb: &Workbook, slot: &str) -> String {
    let Some(p) = wb.preserved.as_deref() else { return String::new() };
    p.elements.iter().filter(|e| e.slot == slot).map(|e| e.xml.as_str()).collect()
}
/// Adds the kept package-root relationships (custom properties…) to `_rels/.rels`.
pub fn write_package_rels(root: &mut Rels, out: &mut Out) {
    for (ty, target, external) in std::mem::take(&mut out.kept.root) {
        root.add_raw(&ty, &target, external);
    }
}
