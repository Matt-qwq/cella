//! Params for a command, generated from its `params` doc string.
//!
//! One generator, two users, so the two never drift: the command-sequence fuzzer
//! (`tests/command_sequence_fuzz.rs`) wants params a command accepts so the run reaches the
//! code inside it, and the per-command timing catalogue (`examples/bench_commands.rs`) wants the
//! same command pointed at the region a user would give it. Everything except the size of that
//! region is identical, including which keys are refused (`path`, `password`, `html`: a command
//! that reads or writes a file has no business in either).
//!
//! A params doc is the `params` string every [`cella_engine::cmd::CommandSpec`] carries, for
//! example `{range?, shift: right|down|row|column, count?}`. [`doc_keys`] reads its top-level
//! keys, and [`doc_params`] picks a value for each one: by the key's name first (`range` is a
//! range, `cell` a cell, `sheet` a sheet), then by the shape of the hint (`asc|desc` is one of
//! those, `bool` a boolean, `1..5` an integer in that range).
//!
//! Each test target that includes this module uses the part it needs (the fuzzer never asks for
//! [`Scale::Whole`]), so the rest is unused there.

#![allow(dead_code)]

use cella_core::{CellRef, MAX_COLS, MAX_ROWS, RangeRef, col_to_letters};
use cella_engine::model::{Table, Workbook};
use serde_json::{Value as Json, json};

// ---------------------------------------------------------------- deterministic randomness

/// SplitMix64: tiny, deterministic and good enough to pick params.
pub struct Rng(u64);

/// The script name the catalogue's prepared workbook saves (`automate.newScript`), so `macro.run`
/// and `automate.runScript` have a real one to run. The fuzzer's sample workbooks carry no script,
/// where those commands are refused either way.
pub const SCRIPT_NAME: &str = "Monthly";

/// The scenario name the catalogue's prepared workbook saves (`data.scenarioAdd`): scenarios live
/// in the workbook's stashed metadata, which this module cannot read.
pub const SCENARIO_NAME: &str = "Best";

/// The extension name the catalogue's prepared workbook saves (`ext.add`).
pub const EXTENSION_NAME: &str = "Refresh";

impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    pub fn below(&mut self, n: usize) -> usize {
        if n == 0 { 0 } else { (self.next() % n as u64) as usize }
    }
    /// Inclusive range.
    pub fn int(&mut self, lo: i64, hi: i64) -> i64 {
        lo + (self.next() % ((hi - lo + 1).max(1) as u64)) as i64
    }
    pub fn chance(&mut self, percent: u64) -> bool {
        self.next() % 100 < percent
    }
    pub fn pick<'a, T>(&mut self, xs: &'a [T]) -> &'a T {
        &xs[self.below(xs.len())]
    }
}

// ---------------------------------------------------------------- coordinates and ranges

/// How big the region-shaped params are.
///
/// The fuzzer uses [`Scale::Default`]: every address is small and near the top-left, so a run
/// of commands stays fast and a formula's references stay cheap to recalculate. The catalogue
/// uses [`Scale::Whole`]: a region-naming key gets the used range of the sheet, which is what
/// "select the column and format it" means for a person, and the only way a command that reads
/// its `range` parameter can show that its cost scales with the data.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Scale {
    #[default]
    Small,
    /// `A1:` the bottom-right corner of `rows` × `cols`.
    Whole { rows: u32, cols: u32 },
}

impl Scale {
    /// A region-shaped parameter under this scale.
    fn range(&self, rng: &mut Rng) -> String {
        // `range` draws from the generator, so it is drawn either way: a command's *other* params
        // then stay the same at both scales for one seed, and a catalogue row differs from the
        // fuzzer's params only in where the region points.
        let small = range(rng);
        match *self {
            Scale::Small => small,
            Scale::Whole { rows, cols } => whole_range(rows, cols),
        }
    }
}

pub fn letter(c: u32) -> String {
    col_to_letters(c)
}

/// A row/column index: mostly near the data, sometimes further out, rarely at the sheet's edge.
fn coord(rng: &mut Rng, near: i64, far: i64, max: u32) -> u32 {
    match rng.below(20) {
        0 => max - 1 - rng.int(0, 2) as u32,
        1 | 2 => rng.int(near, far) as u32,
        _ => rng.int(0, near) as u32,
    }
}

pub fn cell(rng: &mut Rng) -> CellRef {
    CellRef::new(coord(rng, 24, 120, MAX_ROWS), coord(rng, 11, 40, MAX_COLS))
}

/// A cell near the data (formula references stay small).
pub fn near_cell(rng: &mut Rng) -> CellRef {
    CellRef::new(rng.int(0, 22) as u32, rng.int(0, 10) as u32)
}

pub fn near_range(rng: &mut Rng) -> RangeRef {
    let a = near_cell(rng);
    let b = CellRef::new(a.row + rng.int(0, 7) as u32, a.col + rng.int(0, 4) as u32);
    RangeRef::new(a, b)
}

pub fn range(rng: &mut Rng) -> String {
    match rng.below(20) {
        0 | 1 => {
            let c = rng.int(0, 10) as u32;
            format!("{}:{}", letter(c), letter(c + rng.int(0, 2) as u32))
        }
        2 | 3 => {
            let r = rng.int(1, 25);
            format!("{r}:{}", r + rng.int(0, 2))
        }
        4 => cell(rng).a1(),
        5 => {
            let a = cell(rng);
            RangeRef::new(a, a.offset_clamped(rng.int(0, 3), rng.int(0, 3))).a1()
        }
        _ => near_range(rng).a1(),
    }
}

/// `A1:` its bottom-right corner, for `rows` × `cols` from the top-left.
pub fn whole_range(rows: u32, cols: u32) -> String {
    let rows = rows.clamp(1, MAX_ROWS);
    let cols = cols.clamp(1, MAX_COLS);
    format!("A1:{}{rows}", letter(cols - 1))
}

pub fn rows_spec(rng: &mut Rng) -> String {
    let r = coord(rng, 24, 120, MAX_ROWS) as i64 + 1;
    format!("{r}:{}", (r + rng.int(0, 2)).min(MAX_ROWS as i64))
}

pub fn cols_spec(rng: &mut Rng) -> String {
    let c = coord(rng, 11, 40, MAX_COLS);
    format!("{}:{}", letter(c), letter((c + rng.int(0, 1) as u32).min(MAX_COLS - 1)))
}

pub fn quoted(name: &str) -> String {
    format!("'{}'", name.replace('\'', "''"))
}

pub fn spill_anchors(wb: &Workbook) -> Vec<(usize, CellRef)> {
    wb.sheets.iter().enumerate().flat_map(|(i, sh)| sh.spill_ranges.keys().map(move |c| (i, *c))).collect()
}

pub fn tables(wb: &Workbook) -> Vec<(usize, &Table)> {
    wb.sheets.iter().enumerate().flat_map(|(i, sh)| sh.tables.iter().map(move |t| (i, t))).collect()
}

pub fn other_sheet(rng: &mut Rng, wb: &Workbook) -> String {
    quoted(&wb.sheets[rng.below(wb.sheets.len())].name)
}

/// A sheet name as a formula names it: bare when it needs no quoting (`Sheet1`), quoted when it
/// does. A defined name's `refersTo` is parsed rather than evaluated, and there a quoted simple
/// name is not the same thing as a bare one.
pub fn sheet_ref(rng: &mut Rng, wb: &Workbook) -> String {
    let name = wb.sheets[rng.below(wb.sheets.len())].name.clone();
    let bare = !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') && !name.starts_with(|c: char| c.is_ascii_digit());
    if bare { name } else { quoted(&name) }
}

pub fn formula(rng: &mut Rng, wb: &Workbook) -> String {
    let c = near_cell(rng).a1();
    let c2 = near_cell(rng).a1();
    let r = near_range(rng).a1();
    let n = rng.int(1, 6);
    let col = letter(rng.int(0, 9) as u32);
    let tabs = tables(wb);
    match rng.below(30) {
        0 => format!("={c}+{n}"),
        1 => format!("=SUM({r})"),
        2 => format!("={c}*{c2}"),
        3 => format!("={}!{c}*2", other_sheet(rng, wb)),
        4 => format!("=SEQUENCE({n},{})", rng.int(1, 3)),
        5 | 6 if !tabs.is_empty() => {
            let (_, t) = rng.pick(&tabs);
            let column = t.columns.get(rng.below(t.columns.len())).map(|c| c.name.clone()).unwrap_or_default();
            match rng.below(4) {
                0 => format!("=SUM({}[{column}])", t.name),
                1 => format!("=ROWS({})", t.name),
                2 => format!("={}[@{column}]", t.name),
                _ => format!("=COUNTA({}[#All])", t.name),
            }
        }
        7 => format!("=INDIRECT(\"{c}\")"),
        8 => format!("=OFFSET({c},1,1)"),
        9 => format!("=IF({c}>{n},\"big\",{c})"),
        10 => {
            let anchors = spill_anchors(wb);
            match anchors.get(rng.below(anchors.len().max(1))) {
                Some((si, a)) => format!("=SUM({}!{a}#)", quoted(&wb.sheets[*si].name)),
                None => format!("={c}#"),
            }
        }
        11 => format!("=SORT({r})"),
        12 => format!("=UNIQUE({r})"),
        13 => format!("=TRANSPOSE({r})"),
        14 => format!("=COUNTIF({col}:{col},\">0\")"),
        15 => format!("=SUM({}:{}!{c})", other_sheet(rng, wb), other_sheet(rng, wb)),
        16 => format!("=LET(x,{c},x*2)"),
        17 => format!("=XLOOKUP({c},{col}1:{col}20,{col}1:{col}20)"),
        18 => format!("=FILTER({col}1:{col}20,{col}1:{col}20>{n})"),
        19 => match wb.names.get(rng.below(wb.names.len().max(1))) {
            Some(nm) => format!("=SUM({})", nm.name),
            None => format!("={c}"),
        },
        20 => "=ROW()+COLUMN()".into(),
        21 => format!("={c}&\"-\"&{c2}"),
        22 => format!("=AVERAGE({r})*{n}"),
        23 => format!("=MAX({r})-MIN({r})"),
        24 => rng.pick(&["=1/0", "=NA()", "=#REF!+1", "=NOSUCHFN(1)", "=SUM(", "=\"text\""]).to_string(),
        25 => format!("={r}"),
        26 => format!("=INDEX({r},{n},1)"),
        27 => format!("=VLOOKUP({c},{r},2,FALSE)"),
        28 => format!("=SEQUENCE({n})*{c}"),
        _ => format!("=TEXT({c},\"0.00\")"),
    }
}

pub fn literal(rng: &mut Rng) -> String {
    match rng.below(14) {
        0..=3 => rng.int(-50, 500).to_string(),
        4 => format!("{}.{}", rng.int(-9, 99), rng.int(0, 99)),
        5 => rng.pick(&["North", "South", "East", "West", "abc", "Total", "x y"]).to_string(),
        6 => rng.pick(&["TRUE", "FALSE"]).to_string(),
        7 => format!("{}%", rng.int(0, 100)),
        8 => format!("2026-{:02}-{:02}", rng.int(1, 12), rng.int(1, 28)),
        9 => String::new(),
        10 => "'007".into(),
        11 => "$1,234.50".into(),
        12 => "#N/A".into(),
        _ => "1e3".into(),
    }
}

pub fn input(rng: &mut Rng, wb: &Workbook) -> String {
    if rng.chance(55) { formula(rng, wb) } else { literal(rng) }
}

pub fn json_input(rng: &mut Rng, wb: &Workbook) -> Json {
    match rng.below(10) {
        0..=1 => json!(rng.int(-20, 200)),
        2 => Json::Null,
        3 => json!(rng.chance(50)),
        _ => json!(input(rng, wb)),
    }
}

pub fn values_2d(rng: &mut Rng, wb: &Workbook) -> Json {
    let (h, w) = (rng.int(1, 4), rng.int(1, 3));
    Json::Array((0..h).map(|_| Json::Array((0..w).map(|_| json_input(rng, wb)).collect())).collect())
}

pub fn sheet_param(rng: &mut Rng, wb: &Workbook) -> Json {
    match rng.below(10) {
        0 => json!(wb.sheets.len() + rng.below(2)),
        1..=4 => json!(wb.sheets[rng.below(wb.sheets.len())].name),
        _ => json!(rng.below(wb.sheets.len())),
    }
}

pub fn new_name(rng: &mut Rng) -> String {
    rng.pick(&["Data", "Q4 2026", "Summary", "Sheet9", "x", "Sales", "Long name of a sheet 31 chars!", "it's", "Grades"]).to_string()
}

pub fn color(rng: &mut Rng) -> String {
    rng.pick(&["#FF0000", "#00B050", "#FFFF00", "#1F4E79", "none"]).to_string()
}

pub fn table_param(rng: &mut Rng, wb: &Workbook) -> Json {
    let tabs = tables(wb);
    match tabs.get(rng.below(tabs.len() + 1)) {
        Some((_, t)) => json!(t.name),
        None => json!("NoSuchTable"),
    }
}

pub fn chart_ids(wb: &Workbook) -> Vec<u32> {
    wb.sheets.iter().flat_map(|s| s.charts.iter().map(|c| c.id)).collect()
}

// ---------------------------------------------------------------- the workbook's objects

// A command that takes an object by name (`pivot.refresh {pivot}`, `macro.run {name}`) has to be
// given a name the workbook actually holds: generating one prices a lookup that fails. The
// catalogue's prepared workbook carries one of each object for this reason; the fuzzer's sample
// workbooks carry none, and there these return empty and the key falls back to the hint.

pub fn pivot_names(wb: &Workbook) -> Vec<String> {
    wb.sheets.iter().flat_map(|s| s.pivots.iter().map(|p| p.name.clone())).collect()
}

/// The source columns a PivotTable or a table offers: `pivot.addField`, `slicer` tiles and
/// `table.*` name them.
pub fn field_names(wb: &Workbook) -> Vec<String> {
    let mut out: Vec<String> = wb.sheets.iter().flat_map(|s| s.tables.iter().flat_map(|t| t.columns.iter().map(|c| c.name.clone()))).collect();
    out.extend(wb.sheets.iter().flat_map(|s| {
        s.pivots.iter().flat_map(|p| {
            p.rows
                .iter()
                .map(|f| f.source_col.clone())
                .chain(p.columns.iter().map(|f| f.source_col.clone()))
                .chain(p.values.iter().map(|v| v.source_col.clone()))
        })
    }));
    out.sort_unstable();
    out.dedup();
    out
}

/// Slicer names: a PivotTable slicer is named, a table slicer is addressed by id.
pub fn slicer_names(wb: &Workbook) -> Vec<String> {
    wb.sheets.iter().flat_map(|s| s.pivot_slicers.iter().map(|x| x.name.clone())).collect()
}

pub fn table_slicer_ids(wb: &Workbook) -> Vec<u32> {
    wb.sheets.iter().flat_map(|s| s.slicers.iter().map(|x| x.id)).collect()
}

pub fn timeline_names(wb: &Workbook) -> Vec<String> {
    wb.sheets.iter().flat_map(|s| s.pivot_timelines.iter().map(|x| x.name.clone())).collect()
}

pub fn query_names(wb: &Workbook) -> Vec<String> {
    wb.queries.iter().map(|q| q.name.clone()).collect()
}

/// The model tables the workbook holds, as `(name, columns)` — what a relationship links.
pub fn model_tables(wb: &Workbook) -> Vec<(String, Vec<String>)> {
    wb.model.tables.iter().map(|t| (t.name.clone(), t.columns.clone())).collect()
}

pub fn control_ids(wb: &Workbook) -> Vec<u32> {
    wb.sheets.iter().flat_map(|s| s.controls.iter().map(|c| c.id)).collect()
}

/// The controls that hold a number (a slider, a spin button): what `control.setValue` is about.
pub fn number_control_ids(wb: &Workbook) -> Vec<u32> {
    wb.sheets
        .iter()
        .flat_map(|s| s.controls.iter())
        .filter(|c| matches!(c.kind, cella_engine::model::ControlKind::Slider | cella_engine::model::ControlKind::Spin))
        .map(|c| c.id)
        .collect()
}

/// The buttons: what `control.press` is about.
pub fn button_control_ids(wb: &Workbook) -> Vec<u32> {
    wb.sheets.iter().flat_map(|s| s.controls.iter()).filter(|c| matches!(c.kind, cella_engine::model::ControlKind::Button)).map(|c| c.id).collect()
}

pub fn shape_ids(wb: &Workbook) -> Vec<u32> {
    wb.sheets.iter().flat_map(|s| s.shapes.iter().map(|x| x.id)).collect()
}

/// Shapes of one kind, for the commands that only accept that kind (`shape.setText` a text box,
/// `wordArt.set` a WordArt).
pub fn shape_ids_of(wb: &Workbook, kind: cella_engine::model::ShapeKind) -> Vec<u32> {
    wb.sheets.iter().flat_map(|s| s.shapes.iter().filter(|x| x.kind == kind).map(|x| x.id)).collect()
}

pub fn equation_ids(wb: &Workbook) -> Vec<u32> {
    wb.sheets.iter().flat_map(|s| s.equations.iter().map(|x| x.id)).collect()
}

/// The source fields a PivotTable puts on its rows and columns: what `pivot.sort`, `pivot.filter`,
/// `pivot.collapse` and `pivot.group` can act on (a value field is not on an axis).
pub fn pivot_axis_fields(wb: &Workbook) -> Vec<String> {
    let mut v: Vec<String> =
        wb.sheets.iter().flat_map(|s| s.pivots.iter().flat_map(|p| p.rows.iter().chain(p.columns.iter()).map(|f| f.source_col.clone()))).collect();
    v.sort_unstable();
    v.dedup();
    v
}

/// The captions of a PivotTable's value fields (`pivot.valueSettings {field}` takes one).
pub fn value_captions(wb: &Workbook) -> Vec<String> {
    let mut v: Vec<String> = wb.sheets.iter().flat_map(|s| s.pivots.iter().flat_map(|p| p.values.iter().map(|f| f.name.clone()))).collect();
    v.retain(|n| !n.is_empty());
    v.sort_unstable();
    v.dedup();
    v
}

/// The source columns a PivotTable could still add (`pivot.addField`).
pub fn pivot_free_fields(wb: &Workbook) -> Vec<String> {
    let used = pivot_axis_fields(wb);
    let mut v: Vec<String> = field_names(wb).into_iter().filter(|f| !used.contains(f)).collect();
    v.sort_unstable();
    v.dedup();
    v
}

/// The object kinds the workbook holds (`object.delete {kind, id}` names one).
pub fn object_kinds(wb: &Workbook) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    for s in &wb.sheets {
        if !s.charts.is_empty() {
            v.push("chart".into());
        }
        if !s.images.is_empty() {
            v.push("image".into());
        }
        if !s.shapes.is_empty() {
            v.push("shape".into());
        }
        if !s.equations.is_empty() {
            v.push("equation".into());
        }
    }
    v.sort();
    v.dedup();
    v
}

// ---------------------------------------------------------------- params docs

/// Top-level keys of a params doc (`{range?, shift: right|down, count?}`): (key, optional, hint).
pub fn doc_keys(doc: &str) -> Vec<(String, bool, String)> {
    let Some(start) = doc.find('{') else { return vec![] };
    let mut parts = Vec::new();
    let (mut depth, mut in_str, mut cur) = (0i32, false, String::new());
    for ch in doc[start + 1..].chars() {
        match ch {
            '"' => {
                in_str = !in_str;
                cur.push(ch);
            }
            _ if in_str => cur.push(ch),
            '{' | '[' | '(' => {
                depth += 1;
                cur.push(ch);
            }
            '}' | ']' | ')' if depth == 0 => break,
            '}' | ']' | ')' => {
                depth -= 1;
                cur.push(ch);
            }
            ',' if depth == 0 => parts.push(std::mem::take(&mut cur)),
            _ => cur.push(ch),
        }
    }
    parts.push(cur);
    let mut out = Vec::new();
    for part in parts {
        let part = part.trim();
        let key: String = part.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_').collect();
        if key.is_empty() {
            continue;
        }
        let rest = &part[key.len()..];
        let optional = rest.starts_with('?');
        let hint = rest.trim_start_matches('?').trim_start();
        let hint = hint.strip_prefix(':').unwrap_or(hint).trim().to_string();
        out.push((key, optional, hint));
    }
    out
}

/// Bare-word alternatives in a hint (`asc|desc`, `"Good"|"Bad"`), if it is an enumeration.
pub fn alternatives(hint: &str) -> Option<Vec<String>> {
    if !hint.contains('|') {
        return None;
    }
    let alts: Vec<String> = hint
        .split('|')
        .map(|a| a.split(" (").next().unwrap_or(a).trim().trim_matches('"').trim_end_matches('…').to_string())
        .filter(|a| !a.is_empty() && a.chars().all(|c| c.is_ascii_alphanumeric() || " .".contains(c)))
        .collect();
    (!alts.is_empty()).then_some(alts)
}

/// Params from a command's params doc: values chosen by key name, then by the hint's shape.
pub fn doc_params(rng: &mut Rng, wb: &Workbook, cmd: &str, doc: &str, scale: Scale) -> Json {
    let mut p = serde_json::Map::new();
    for (key, optional, hint) in doc_keys(doc) {
        // A key the doc marks optional but the command cannot run without: the fuzzer wants the
        // odd refusal, the catalogue is measuring the command and "it said no" is not a cost.
        if omitted_key(cmd, &key) {
            continue;
        }
        // A region-naming key is always given. Without it the command works on the *selection*,
        // which is the same one cell at every scale: "Clear All" then reads 1 microsecond on a
        // million-row sheet and 9 seconds on the same cells spread over a thousand columns, and
        // the two rows are not the same question. The scale's meaning is the region it names.
        if optional && !forced_key(cmd, &key) && !names_a_region(&key) && rng.chance(40) {
            continue;
        }
        if let Some(v) = doc_value(rng, wb, cmd, &key, &hint, scale) {
            p.insert(key, v);
        }
    }
    // Keys the doc does not name but the command needs: the Analysis ToolPak takes a `range` its
    // doc only describes in prose, so without this the command is always "missing `range`".
    for (key, value) in extra_params(cmd, wb, rng, scale) {
        p.insert(key, value);
    }
    Json::Object(p)
}

/// Params a command takes that its `params` doc does not spell out as keys.
fn extra_params(cmd: &str, _wb: &Workbook, _rng: &mut Rng, scale: Scale) -> Vec<(String, serde_json::Value)> {
    match cmd {
        // Descriptive Statistics is the tool with the simplest inputs: one range, a labels row and
        // an output cell. (The others need two ranges, known variances, or a bins range.)
        "data.analysisToolPak" => vec![
            ("tool".to_string(), json!("descriptive")),
            // One numeric column, header row included: the tool refuses a range that mixes text in.
            // Column B is the sheet's numbers (column A is the item names).
            (
                "range".to_string(),
                json!(match scale {
                    Scale::Small => "B1:B11".to_string(),
                    Scale::Whole { rows, .. } => format!("B1:B{rows}"),
                }),
            ),
            ("output".to_string(), json!("P1")),
            // The range starts at the header row: without this the tool reads "Id" as a number.
            ("labels".to_string(), json!(true)),
        ],
        _ => Vec::new(),
    }
}

/// Whether the key names the region a command works on, so the scale means something.
fn names_a_region(key: &str) -> bool {
    matches!(key.to_ascii_lowercase().as_str(), "range" | "ranges")
}

/// Keys a command needs even though its doc marks them optional.
///
/// The catalogue runs a command alone, so a command whose optional key is the one that makes it do
/// work is a command with no number. Listing it here (rather than never skipping optionals at all)
/// keeps the fuzzer's deliberately thin params for everything else.
fn forced_key(cmd: &str, key: &str) -> bool {
    let k = key.to_ascii_lowercase();
    matches!(
        (cmd, k.as_str()),
        // A Timeline needs the PivotTable it filters; without one the command asks the user to
        // select a cell in a report and does nothing.
        ("insert.timeline", "pivot")
            // A Data Table needs at least one input cell; both are optional because either will do.
            | ("data.dataTable", "rowinput")
            | ("data.dataTable", "columninput")
            // A point's label is addressed by series and point: `text` alone is refused.
            | ("chart.setDataLabels", "series")
            | ("chart.setDataLabels", "point")
    )
}

/// Keys whose absence is the *valid* call, per command:
/// - `pageLayout.themeColors` recolours the workbook's own theme when given no name, and a name the
///   generator invents is an error.
/// - `chart.setDataLabels` edits every series' labels unless given a point, and a point needs a
///   series and excludes `deletePoint`/`showPoint` and a per-point `text`: all four at once is
///   contradictory (the command says so), so the catalogue asks for the all-series form.
/// - `data.query.add` names the new query itself, and the prepared workbook's query already holds
///   the name the generator would otherwise reuse.
/// - `data.solverReport` reads the Solver model the workbook saved; its `values`/`original` maps are
///   cell-to-number objects whose cells must be that model's variables.
fn omitted_key(cmd: &str, key: &str) -> bool {
    let k = key.to_ascii_lowercase();
    (cmd == "pageLayout.themeColors" && k == "name")
        || (cmd == "chart.setDataLabels" && matches!(k.as_str(), "text" | "point" | "deletepoint" | "showpoint"))
        || (cmd == "data.query.add" && k == "name")
        || (cmd == "data.solverReport" && matches!(k.as_str(), "model" | "values" | "original"))
        // A Find is the call that scans and reports the next match; `all` is Find All, which builds
        // one JSON object per match — at a million hits the row would time the JSON, not the scan.
        // `byColumns` sorts the hits before picking one, another cost that is not the search.
        || (cmd == "edit.find" && matches!(k.as_str(), "all" | "bycolumns"))
}

/// An object the workbook holds, by the name or id its command takes — consulted before the generic
/// rules, because `{name}` means a script to `macro.run` and a new sheet to `sheet.rename`, and
/// `{field}` means a row field to `pivot.sort` and an unused column to `pivot.addField`.
fn object_value(cmd: &str, key: &str, wb: &Workbook, rng: &mut Rng) -> Option<Json> {
    let mut pick = |list: Vec<String>| -> Option<Json> { if list.is_empty() { None } else { Some(json!(list[rng.below(list.len())].clone())) } };
    let first = |ids: Vec<u32>| -> Option<Json> { ids.first().map(|id| json!(id)) };
    let k = key.to_ascii_lowercase();
    match cmd {
        // A search that finds nothing is an error, and a row the command said no to is a row with
        // no number. The needle is a word every prepared sheet holds (its region column), and the
        // search spans the workbook: the scan whose cost follows columns and sheets.
        "edit.find" if k == "what" => Some(json!("North")),
        "edit.find" if k == "within" => Some(json!("workbook")),
        "insert.pivotChart" | "insert.slicer" | "insert.timeline" | "table.insertSlicer" | "table.summarizePivot" | "insert.pivotTable"
            if k == "pivot" || k == "field" || k == "slicer" || k == "timeline" || k == "style" =>
        {
            match k.as_str() {
                "pivot" => pick(pivot_names(wb)),
                // A Timeline filters on a date field, and the prepared PivotTable's axis fields
                // (a region, a quantity) are not dates: the source's Date column is what carries one.
                "field" if cmd == "insert.timeline" => Some(json!("Date")),
                "field" => pick(pivot_axis_fields(wb)),
                "slicer" => pick(slicer_names(wb)),
                "timeline" => pick(timeline_names(wb)),
                _ => Some(json!("PivotStyleLight16")),
            }
        }
        c if c.starts_with("pivot.") => match k.as_str() {
            "pivot" => pick(pivot_names(wb)),
            // A field the command can act on: a row or column field, or for `addField` one that is
            // not on an axis yet.
            "field" if c == "pivot.addField" => pick(pivot_free_fields(wb)),
            "field" if c == "pivot.valueSettings" => pick(value_captions(wb)).or(Some(json!("Sum of Qty"))),
            "field" => pick(pivot_axis_fields(wb)),
            "style" => Some(json!("PivotStyleLight16")),
            "selected" => Some(json!(["North"])),
            "value" if c == "pivot.filterByValue" => Some(json!(1)),
            "value" => Some(json!("North")),
            // `moveField`'s `from`/`to` are areas, not a destination cell.
            "from" if c == "pivot.moveField" => Some(json!("rows")),
            "to" if c == "pivot.moveField" => Some(json!("columns")),
            _ => None,
        },
        c if c.starts_with("slicer.") => match k.as_str() {
            "slicer" => pick(slicer_names(wb)),
            "timeline" => pick(timeline_names(wb)),
            "style" => Some(json!("SlicerStyleLight1")),
            "value" => Some(json!("North")),
            "selected" => Some(json!(["North", "South"])),
            _ => None,
        },
        c if c.starts_with("macro.") || c.starts_with("automate.") => match k.as_str() {
            "name" | "script" => Some(json!(SCRIPT_NAME)),
            "to" => Some(json!(format!("{SCRIPT_NAME} 2"))),
            // The script the prepared workbook saved holds one step, so its index is 0.
            "index" => Some(json!(0)),
            "command" => Some(json!("cell.set")),
            _ => None,
        },
        "ext.add" | "ext.run" | "ext.remove" => match k.as_str() {
            "name" => Some(json!(EXTENSION_NAME)),
            _ => None,
        },
        "data.scenarioShow" | "data.scenarioDelete" => match k.as_str() {
            "name" | "scenario" => Some(json!(SCENARIO_NAME)),
            _ => None,
        },
        c if c.starts_with("data.query.") => match k.as_str() {
            "name" | "query" => pick(query_names(wb)),
            // A query source is a tagged object: a table the workbook carries is the one source the
            // prepared workbook can always serve.
            "source" => Some(json!({
                "kind": "table",
                "table": tables(wb).first().map(|(_, t)| t.name.clone()).unwrap_or_else(|| "Sales".to_string()),
            })),
            // A step is a serde enum: the unit variant is its name, the rest are objects.
            "step" => Some(json!("PromoteHeaders")),
            "steps" => Some(json!(["PromoteHeaders"])),
            "index" => Some(json!(0)),
            _ => None,
        },
        c if c.starts_with("control.") => match k.as_str() {
            // A button has no value; the slider the prepared workbook placed does. `press` is the
            // other way round.
            "id" | "control" if c == "control.press" => button_control_ids(wb).first().map(|id| json!(id)),
            "id" | "control" if c == "control.setValue" => number_control_ids(wb).first().map(|id| json!(id)),
            "id" | "control" => control_ids(wb).first().map(|id| json!(id)),
            "value" if c == "control.setValue" => Some(json!(5)),
            _ => None,
        },
        // A field that is a whole number in one command is an outline level in another.
        "data.showLevels" => match k.as_str() {
            "rows" | "cols" => Some(json!(1)),
            _ => None,
        },
        "data.goalSeek" if k == "to" => Some(json!(100)),
        "data.dataTable" if k == "rowinput" || k == "columninput" => Some(json!("B1")),
        "sheet.move" if k == "to" => Some(json!(0)),
        "pivot.moveField" => match k.as_str() {
            "from" => Some(json!("rows")),
            "to" => Some(json!("columns")),
            _ => None,
        },
        "pivot.valueSettings" if k == "field" => pick(value_captions(wb)),
        "pivot.filterByValue" if k == "value" => Some(json!(1)),
        "pivot.filterByLabel" if k == "value" => Some(json!("North")),
        "sheet.moveColumns" if k == "before" => Some(json!(0)),
        "insert.wordArt" | "wordArt.set" => match k.as_str() {
            "style" => Some(json!("Classic")),
            "fill" => Some(json!("#1F4E79")),
            "outline" => Some(json!(false)),
            "shadow" => Some(json!(false)),
            "id" | "shape" | "object" => first(shape_ids_of(wb, cella_engine::model::ShapeKind::WordArt)),
            _ => None,
        },
        "object.format" => match k.as_str() {
            "kind" => Some(json!("shape")),
            "bold" | "italic" => Some(json!(true)),
            // A typeface name, not a bool: the hint says `typeface` and the generic rules read any
            // unrecognised hint as a number.
            "font" => Some(json!("Arial")),
            // Colours: the hints are `#RRGGBB|{theme, tint}|none` and the generic rules read them as
            // a range; the outline is an object of its own.
            "line" | "text_color" | "fill" => Some(json!("#1F4E79")),
            "text_outline" => Some(json!({"color": "#1F4E79", "width": 1})),
            "alt" => Some(json!("an object")),
            // A shape is addressed by id, and a chart id names no shape.
            "id" => first(shape_ids(wb)),
            _ => None,
        },
        // A whole `Style` is a struct of structs; a partial patch is what the command takes, and
        // the number format is the one part whose JSON is a plain string.
        "home.formatCells" if k == "style" => Some(json!({"numFmt": "0.00"})),
        // A point's marker takes a colour, not a bool (its hint is `hex|"none"`).
        "chart.formatPoint" if k == "marker" => Some(json!("#C00000")),
        // `chart.moveToObject` moves a chart *off* a chart sheet, so it wants the chart that
        // prepare moved there — the last one it created.
        "chart.moveToObject" if k == "chart" => chart_ids(wb).last().map(|id| json!(id)),
        // A report is a list of kinds, not a table column: the generic rules read the hint's
        // square brackets as a column name.
        "data.solverReport" if k == "reports" => Some(json!(["answer"])),
        // A relationship links two model tables on a column they share; the prepared workbook has
        // two (`SalesQuery` and `Regions`) and both carry `Region`.
        c if c.starts_with("data.relationship.") => match k.as_str() {
            "from" | "to" => {
                let tables = model_tables(wb);
                let pick = if k == "from" { 0 } else { 1 };
                let (name, columns) = tables.get(pick).or_else(|| tables.first())?;
                let shared = columns.iter().find(|c| c.eq_ignore_ascii_case("Region"));
                Some(json!({"table": name, "column": shared.or_else(|| columns.first()).cloned().unwrap_or_default()}))
            }
            _ => None,
        },
        // Every chart command addresses a chart by id, and the prepared workbook has one: an id
        // drawn at random names no chart and the command answers "no chart on this sheet".
        c if c.starts_with("chart.") && k == "chart" => first(chart_ids(wb)),
        "automate.appendStep" | "automate.editStep" if k == "params" => Some(json!({})),
        "insert.timeline" if k == "field" => Some(json!("Date")),
        "data.consolidate" if k == "sources" => Some(json!(["Sheet1!$A$1:$B$10"])),
        "formulas.defineName" if k == "scope" => Some(json!("Workbook")),
        // `oldName` means "edit that name", and a name that is not there is an error: the catalogue
        // wants the define path, not the edit one.
        "formulas.defineName" if k == "oldname" => Some(Json::Null),
        "formulas.deleteName" => match k.as_str() {
            "name" => Some(json!("Data")),
            "scope" => Some(json!("Workbook")),
            _ => None,
        },
        "shape.setText" => match k.as_str() {
            "id" | "shape" | "object" => first(shape_ids_of(wb, cella_engine::model::ShapeKind::TextBox)),
            _ => None,
        },
        "equation.setText" => match k.as_str() {
            "id" | "shape" | "object" => first(equation_ids(wb)),
            _ => None,
        },
        c if c.starts_with("object.") || c.starts_with("arrange.") => match k.as_str() {
            // The kind and the id have to name the same object, so both come from the shape the
            // prepared workbook placed first.
            "kind" if !shape_ids(wb).is_empty() => Some(json!("shape")),
            "kind" => pick(object_kinds(wb)),
            "id" | "shape" | "object" => first(shape_ids(wb)).or_else(|| first(equation_ids(wb))).or_else(|| first(chart_ids(wb))),
            _ => None,
        },
        c if c.starts_with("chart.") => match k.as_str() {
            "series" => Some(json!(1)),
            "point" => Some(json!(1)),
            // A chart element's colours: what `formatPoint` and `setDataLabels` type.
            "fill" => Some(json!("#1F4E79")),
            "line" => Some(json!("#FF0000")),
            "on" => Some(json!(true)),
            _ => None,
        },
        // J5 = SUM(B2:B10) has precedents and is the precedent of J6, so either cell answers one.
        "formulas.selectPrecedents" if k == "cell" => Some(json!("J5")),
        "formulas.selectDependents" if k == "cell" => Some(json!("J5")),
        _ => None,
    }
}

fn doc_value(rng: &mut Rng, wb: &Workbook, cmd: &str, key: &str, hint: &str, scale: Scale) -> Option<Json> {
    if let Some(v) = object_value(cmd, key, wb, rng) {
        return Some(v);
    }
    let lower = key.to_ascii_lowercase();
    let v = match lower.as_str() {
        "path" | "base64" | "password" | "html" => return None,
        "range" | "source" | "target" | "destination" | "criteria" | "copyto" | "printarea" | "add" | "remove" | "apply" => {
            json!(scale.range(rng))
        }
        // Single cells that the old rules put in the region list: a Sparkline's `location`, a
        // Goal Seek's `changing` cell and a data table's input cells are cells, not ranges.
        "location" | "changing" | "rowinput" | "columninput" => json!(near_cell(rng).a1()),
        "refersto" => json!(format!("={}!$A$1:$B$4", sheet_ref(rng, wb))),
        "cell" | "at" | "active" | "fromcell" | "set" | "reference" => json!(cell(rng).a1()),
        "rows" | "titlerows" if !hint.starts_with('[') => json!(rows_spec(rng)),
        "cols" | "titlecols" if !hint.starts_with('[') => json!(cols_spec(rng)),
        "sheet" => sheet_param(rng, wb),
        "input" | "formula" | "formula1" | "formula2" | "value" | "value2" | "with" => json!(input(rng, wb)),
        "table" => table_param(rng, wb),
        "chart" | "id" => {
            let ids = chart_ids(wb);
            json!(ids.get(rng.below(ids.len() + 1)).copied().unwrap_or(rng.int(0, 5) as u32))
        }
        "ids" => json!(chart_ids(wb)),
        "color" => json!(color(rng)),
        "values" if hint.starts_with("[[") => values_2d(rng, wb),
        "keys" => json!([{"column": letter(rng.int(0, 9) as u32), "order": rng.pick(&["asc", "desc"])}]),
        "column" | "groupby" => json!(letter(rng.int(0, 7) as u32)),
        "columns" | "sources" if !hint.contains('|') => json!([letter(rng.int(0, 7) as u32)]),
        "points" => json!([[10, 10], [40, 25], [70, 5]]),
        "commands" => json!([{"command": "cell.set", "params": {"cell": near_cell(rng).a1(), "input": input(rng, wb)}}]),
        "char" => json!("€"),
        "level" => json!(1),
        "confidence" => json!(0.95),
        // A destination: a cell for the commands that move something, a name for the ones that
        // rename (those are in `object_value`, which runs first).
        "to" => json!(near_cell(rng).a1()),
        // The workbook's objects, by the names it gave them. A key whose list is empty (the
        // fuzzer's sample workbooks carry no PivotTable) falls through to the hint, which is the
        // old behaviour.
        "pivot" if !pivot_names(wb).is_empty() => json!(rng.pick(&pivot_names(wb))),
        "field" if !field_names(wb).is_empty() => json!(rng.pick(&field_names(wb))),
        "slicer" if !slicer_names(wb).is_empty() => json!(rng.pick(&slicer_names(wb))),
        "timeline" if !timeline_names(wb).is_empty() => json!(rng.pick(&timeline_names(wb))),
        "query" if !query_names(wb).is_empty() => json!(rng.pick(&query_names(wb))),
        "control" if !control_ids(wb).is_empty() => json!(rng.pick(&control_ids(wb))),
        "shape" if !shape_ids(wb).is_empty() => json!(rng.pick(&shape_ids(wb))),
        "script" => json!(SCRIPT_NAME),
        "height" | "width" => json!(rng.int(40, 200)),
        "before" => json!(letter(rng.int(0, 10) as u32)),
        "name" | "oldname" if alternatives(hint).is_none() => json!(new_name(rng)),
        "text" | "title" | "comment" | "author" | "word" | "header" | "footer" | "what" if alternatives(hint).is_none() => {
            json!(rng.pick(&["North", "note", "&CPage &P", "x"]))
        }
        _ => return Some(hint_value(rng, wb, hint)),
    };
    Some(v)
}

fn hint_value(rng: &mut Rng, wb: &Workbook, hint: &str) -> Json {
    if let Some(alts) = alternatives(hint) {
        return json!(rng.pick(&alts));
    }
    if hint.contains("bool") || hint == "true" || hint == "false" {
        return json!(rng.chance(50));
    }
    if let Some((a, b)) = hint.split_once("..")
        && let (Ok(a), Ok(b)) = (a.trim().parse::<i64>(), b.trim().split(' ').next().unwrap_or("").parse::<i64>())
    {
        return json!(rng.int(a, b));
    }
    if let Ok(n) = hint.split(' ').next().unwrap_or("").parse::<f64>() {
        return json!((n * rng.int(0, 3) as f64).round());
    }
    if hint.starts_with('[') {
        let heads: Vec<String> = tables(wb).iter().flat_map(|(_, t)| t.columns.iter().map(|c| c.name.clone())).collect();
        return match heads.get(rng.below(heads.len() + 1)) {
            Some(h) => json!([h]),
            None => json!([]),
        };
    }
    if let Some(ex) = hint.strip_prefix('"').and_then(|h| h.split('"').next())
        && rng.chance(60)
    {
        return json!(ex);
    }
    match rng.below(4) {
        0 => json!(rng.chance(50)),
        1 => json!(rng.int(0, 5)),
        2 => json!(near_range(rng).a1()),
        _ => json!(literal(rng)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn docs_parse_into_keys() {
        let keys = doc_keys("{range?, shift: right|down|row|column, input: \"text, number or =formula\"}");
        let names: Vec<_> = keys.iter().map(|(k, o, _)| (k.as_str(), *o)).collect();
        assert_eq!(names, [("range", true), ("shift", false), ("input", false)]);
        assert_eq!(alternatives(&keys[1].2), Some(vec!["right".into(), "down".into(), "row".into(), "column".into()]));
    }

    /// The catalogue's one difference: a region-naming key covers the whole used area.
    #[test]
    fn whole_scale_points_a_range_at_the_used_area() {
        let wb = Workbook::default();
        let doc = "{range: A1:F, header?}";
        let small = doc_params(&mut Rng::new(1), &wb, "home.bold", doc, Scale::Small);
        let whole = doc_params(&mut Rng::new(1), &wb, "home.bold", doc, Scale::Whole { rows: 1_000_000, cols: 6 });
        assert_eq!(whole["range"], json!("A1:F1000000"));
        // The same seed gives the same non-region keys, so a row of the catalogue differs from the
        // fuzzer's params only in where it points.
        assert_eq!(small.get("header").is_some(), whole.get("header").is_some());
        assert_ne!(small["range"], whole["range"]);
        assert_eq!(whole_range(100_000, 6), "A1:F100000");
        // Hostile sizes are clamped to the sheet, never 0 or past its edge.
        assert_eq!(whole_range(0, 0), "A1:A1");
    }
}
